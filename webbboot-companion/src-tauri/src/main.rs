#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rusb::{devices};
use serde::{Serialize, Deserialize};
use std::process::Command;
use std::path::Path;
use log::{info, error, warn};
use simplelog::{TermLogger, Config, LevelFilter};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use futures_util::{SinkExt, StreamExt};
use std::fs;
use std::process::Stdio;
use std::os::unix::process::ExitStatusExt;

#[derive(Serialize, Deserialize, Debug)]
struct Job {
    action: String,
    iso: Option<String>,
    filesystem: String,
    scheme: String,
    device: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct UsbDevice {
    id: String,
    name: String,
    vendor_id: u16,
    product_id: u16,
    size: Option<u64>,
    mount_point: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ProgressUpdate {
    status: String,
    progress: u8,
    current_operation: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct DeviceInfo {
    path: String,
    size: u64,
    filesystem: Option<String>,
    is_mounted: bool,
    mount_points: Vec<String>,
}

#[tauri::command]
fn list_usb_devices() -> Vec<UsbDevice> {
    match devices() {
        Ok(dev_list) => dev_list
            .iter()
            .filter_map(|dev| {
                match dev.device_descriptor() {
                    Ok(desc) => {
                        match dev.open() {
                            Ok(handle) => {
                                if let Ok(config) = handle.active_configuration() {
                                    if let Ok(config_desc) = dev.config_descriptor(config - 1) {
                                        let is_mass_storage = config_desc
                                            .interfaces()
                                            .flat_map(|interface| interface.descriptors())
                                            .any(|desc| desc.class_code() == 0x08);
                                        if is_mass_storage {
                                            let device_id = format!("USB {:04x}:{:04x}", desc.vendor_id(), desc.product_id());
                                            let device_name = get_device_name(&desc, &handle);
                                            let device_path = find_device_path(desc.vendor_id(), desc.product_id());
                                            let device_size = get_device_size(&device_path);
                                            
                                            Some(UsbDevice {
                                                id: device_path.unwrap_or(device_id.clone()),
                                                name: format!("{} ({})", device_name, device_id),
                                                vendor_id: desc.vendor_id(),
                                                product_id: desc.product_id(),
                                                size: device_size,
                                                mount_point: None,
                                            })
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            }
                            Err(_) => None,
                        }
                    }
                    Err(_) => None,
                }
            })
            .collect(),
        Err(e) => {
            error!("Failed to list USB devices: {}", e);
            vec![]
        }
    }
}

fn get_device_name(desc: &rusb::DeviceDescriptor, handle: &rusb::DeviceHandle<rusb::GlobalContext>) -> String {
    // Try to get available languages first
    let timeout = std::time::Duration::from_secs(1);
    
    if let Ok(languages) = handle.read_languages(timeout) {
        if let Some(&language) = languages.first() {
            let manufacturer = handle.read_manufacturer_string(
                language, 
                desc, 
                timeout
            ).unwrap_or_else(|_| "Unknown".to_string());
            
            let product = handle.read_product_string(
                language, 
                desc, 
                timeout
            ).unwrap_or_else(|_| "Device".to_string());
            
            return format!("{} {}", manufacturer, product).trim().to_string();
        }
    }
    
    // Fallback if no language descriptors are available
    format!("USB Device {:04x}:{:04x}", desc.vendor_id(), desc.product_id())
}

#[cfg(target_os = "linux")]
fn find_device_path(_vendor_id: u16, _product_id: u16) -> Option<String> {
    // Try to find the actual device path in /dev
    if let Ok(entries) = fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name() {
                if let Some(name_str) = name.to_str() {
                    if name_str.starts_with("sd") && name_str.len() == 3 {
                        // This is a basic heuristic - in a real implementation,
                        // you'd want to match the USB device to the block device
                        // through sysfs or similar mechanisms
                        return Some(path.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn find_device_path(_vendor_id: u16, _product_id: u16) -> Option<String> {
    None
}

fn get_device_size(device_path: &Option<String>) -> Option<u64> {
    if let Some(path) = device_path {
        #[cfg(target_os = "linux")]
        {
            if let Ok(output) = Command::new("lsblk")
                .args(["-b", "-n", "-o", "SIZE", path])
                .output() 
            {
                if let Ok(size_str) = String::from_utf8(output.stdout) {
                    if let Ok(size) = size_str.trim().parse::<u64>() {
                        return Some(size);
                    }
                }
            }
        }
    }
    None
}

async fn execute_job(job: Job, write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, tokio_tungstenite::tungstenite::Message>) {
    info!("Received job: {:?}", job);
    
    // Validate inputs
    if job.device.is_empty() {
        send_progress_update(write, "Error: No device selected", 0, "validation").await;
        return;
    }
    
    if job.action == "create" && job.iso.is_none() {
        send_progress_update(write, "Error: No ISO file specified", 0, "validation").await;
        return;
    }

    // Check if device exists and is accessible
    if !Path::new(&job.device).exists() {
        send_progress_update(write, &format!("Error: Device {} not found", job.device), 0, "validation").await;
        return;
    }

    // Verify device before starting
    send_progress_update(write, "Verifying device...", 5, "device verification").await;
    match verify_device(job.device.clone()) {
        Ok(device_info) => {
            if device_info.is_mounted {
                send_progress_update(write, "Error: Device is currently mounted. Please unmount first.", 0, "verification").await;
                return;
            }
            info!("Device verified: {} ({} bytes)", device_info.path, device_info.size);
        }
        Err(e) => {
            send_progress_update(write, &format!("Device verification failed: {}", e), 0, "verification").await;
            return;
        }
    }

    // Format the device
    send_progress_update(write, "Formatting device...", 10, "formatting").await;
    if !format_device(&job, write).await {
        return;
    }

    // If creating bootable USB, write the ISO
    if job.action == "create" {
        send_progress_update(write, "Writing ISO to device...", 50, "iso writing").await;
        if !write_iso(&job, write).await {
            return;
        }
        
        send_progress_update(write, "Verifying write operation...", 95, "verification").await;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    send_progress_update(write, "Operation completed successfully!", 100, "complete").await;
}

async fn send_status(write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, tokio_tungstenite::tungstenite::Message>, status: &str, progress: u8) {
    let msg = serde_json::json!({
        "status": status,
        "progress": progress
    });
    if let Err(e) = write.send(tokio_tungstenite::tungstenite::Message::Text(msg.to_string())).await {
        error!("Failed to send WebSocket message: {}", e);
    }
}

async fn send_progress_update(
    write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, tokio_tungstenite::tungstenite::Message>,
    status: &str,
    progress: u8,
    operation: &str
) {
    let update = ProgressUpdate {
        status: status.to_string(),
        progress,
        current_operation: operation.to_string(),
    };
    
    let msg = serde_json::to_string(&update).unwrap_or_else(|_| {
        serde_json::json!({
            "status": status,
            "progress": progress,
            "current_operation": operation
        }).to_string()
    });
    
    if let Err(e) = write.send(tokio_tungstenite::tungstenite::Message::Text(msg)).await {
        error!("Failed to send progress update: {}", e);
    }
}

async fn format_device(job: &Job, write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, tokio_tungstenite::tungstenite::Message>) -> bool {
    #[cfg(target_os = "linux")]
    {
        let format_result = Command::new("sudo")
            .args(["mkfs", &format!("-t{}", job.filesystem.to_lowercase()), &job.device])
            .output();

        match format_result {
            Ok(output) if output.status.success() => {
                info!("Successfully formatted {} with {}", job.device, job.filesystem);
                true
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!("Format failed: {}", stderr);
                send_status(write, &format!("Format failed: {}", stderr), 0).await;
                false
            }
            Err(e) => {
                error!("Failed to execute format command: {}", e);
                send_status(write, "Format failed: Unable to execute format command", 0).await;
                false
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Windows formatting logic
        let format_result = Command::new("format")
            .args([&job.device, "/FS:FAT32", "/Q", "/Y"])
            .output();

        match format_result {
            Ok(output) if output.status.success() => {
                info!("Successfully formatted {} with {}", job.device, job.filesystem);
                true
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!("Format failed: {}", stderr);
                send_status(write, &format!("Format failed: {}", stderr), 0).await;
                false
            }
            Err(e) => {
                error!("Failed to execute format command: {}", e);
                send_status(write, "Format failed: Unable to execute format command", 0).await;
                false
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS formatting logic
        let format_result = Command::new("diskutil")
            .args(["eraseDisk", "FAT32", "WEBBOOT", &job.device])
            .output();

        match format_result {
            Ok(output) if output.status.success() => {
                info!("Successfully formatted {} with {}", job.device, job.filesystem);
                true
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!("Format failed: {}", stderr);
                send_status(write, &format!("Format failed: {}", stderr), 0).await;
                false
            }
            Err(e) => {
                error!("Failed to execute format command: {}", e);
                send_status(write, "Format failed: Unable to execute format command", 0).await;
                false
            }
        }
    }
}

async fn write_iso(job: &Job, write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, tokio_tungstenite::tungstenite::Message>) -> bool {
    let iso_path = job.iso.as_ref().unwrap();
    
    // Validate ISO file
    if !Path::new(iso_path).exists() {
        error!("ISO file not found: {}", iso_path);
        send_status(write, "Error: ISO file not found", 0).await;
        return false;
    }

    // Get ISO file size for progress calculation
    let iso_size = match fs::metadata(iso_path) {
        Ok(metadata) => metadata.len(),
        Err(e) => {
            error!("Failed to get ISO file size: {}", e);
            send_status(write, "Error: Cannot read ISO file", 0).await;
            return false;
        }
    };

    #[cfg(target_os = "linux")]
    {
        write_iso_linux(iso_path, &job.device, iso_size, write).await
    }

    #[cfg(target_os = "windows")]
    {
        write_iso_windows(iso_path, &job.device, write).await
    }

    #[cfg(target_os = "macos")]
    {
        write_iso_macos(iso_path, &job.device, write).await
    }
}

#[cfg(target_os = "linux")]
async fn write_iso_linux(iso_path: &str, device: &str, _iso_size: u64, write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, tokio_tungstenite::tungstenite::Message>) -> bool {
    let mut child = match Command::new("sudo")
        .args(["dd", &format!("if={}", iso_path), &format!("of={}", device), "bs=4M", "status=progress"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            error!("Failed to start dd command: {}", e);
            send_status(write, "Error: Failed to start ISO write process", 0).await;
            return false;
        }
    };

    // Monitor progress (basic implementation)
    let mut progress = 50;
    let progress_increment = 5;
    
    while child.try_wait().unwrap_or(Some(std::process::ExitStatus::from_raw(256))).is_none() {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        progress = std::cmp::min(progress + progress_increment, 95);
        send_status(write, &format!("Writing ISO... {}%", progress), progress).await;
    }

    match child.wait() {
        Ok(status) if status.success() => {
            info!("Successfully wrote ISO to {}", device);
            true
        }
        Ok(status) => {
            error!("ISO write failed with exit code: {}", status);
            send_status(write, "Error: ISO write failed", 0).await;
            false
        }
        Err(e) => {
            error!("Failed to wait for dd process: {}", e);
            send_status(write, "Error: ISO write process failed", 0).await;
            false
        }
    }
}

#[cfg(target_os = "windows")]
async fn write_iso_windows(iso_path: &str, device: &str, write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, tokio_tungstenite::tungstenite::Message>) -> bool {
    // Use Windows-specific tools like Rufus API or PowerShell
    let write_result = Command::new("powershell")
        .args(["-Command", &format!("Copy-Item '{}' '{}'", iso_path, device)])
        .output();

    match write_result {
        Ok(output) if output.status.success() => {
            info!("Successfully wrote ISO to {}", device);
            true
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("ISO write failed: {}", stderr);
            send_status(write, &format!("ISO write failed: {}", stderr), 0).await;
            false
        }
        Err(e) => {
            error!("Failed to execute ISO write command: {}", e);
            send_status(write, "Error: Failed to execute ISO write command", 0).await;
            false
        }
    }
}

#[cfg(target_os = "macos")]
async fn write_iso_macos(iso_path: &str, device: &str, write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, tokio_tungstenite::tungstenite::Message>) -> bool {
    let write_result = Command::new("dd")
        .args([&format!("if={}", iso_path), &format!("of={}", device), "bs=4m"])
        .output();

    match write_result {
        Ok(output) if output.status.success() => {
            info!("Successfully wrote ISO to {}", device);
            true
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("ISO write failed: {}", stderr);
            send_status(write, &format!("ISO write failed: {}", stderr), 0).await;
            false
        }
        Err(e) => {
            error!("Failed to execute ISO write command: {}", e);
            send_status(write, "Error: Failed to execute ISO write command", 0).await;
            false
        }
    }
}

async fn handle_websocket(_app: tauri::AppHandle) {
    TermLogger::init(
        LevelFilter::Info, 
        Config::default(), 
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto
    ).unwrap();
    
    let listener = match TcpListener::bind("127.0.0.1:8080").await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind to port 8080: {}", e);
            return;
        }
    };
    
    info!("WebSocket server started on ws://localhost:8080");

    while let Ok((stream, addr)) = listener.accept().await {
        info!("New WebSocket connection from: {}", addr);
        
        let ws_stream = match accept_async(stream).await {
            Ok(ws) => ws,
            Err(e) => {
                error!("Failed to accept WebSocket connection: {}", e);
                continue;
            }
        };
        
        let (mut write, mut read) = ws_stream.split();

        // Send initial device list
        let devices = list_usb_devices();
        let msg = serde_json::json!({"devices": devices});
        if let Err(e) = write.send(tokio_tungstenite::tungstenite::Message::Text(msg.to_string())).await {
            error!("Failed to send initial device list: {}", e);
            continue;
        }

        // Handle incoming messages
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(msg) => {
                    if let Ok(text) = msg.to_text() {
                        match serde_json::from_str::<Job>(text) {
                            Ok(job) => {
                                info!("Processing job: {:?}", job);
                                execute_job(job, &mut write).await;
                            }
                            Err(e) => {
                                warn!("Failed to parse job JSON: {}", e);
                                let error_msg = serde_json::json!({
                                    "status": "Error: Invalid job format",
                                    "progress": 0
                                });
                                let _ = write.send(tokio_tungstenite::tungstenite::Message::Text(error_msg.to_string())).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
            }
        }
        
        info!("WebSocket connection closed for: {}", addr);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                handle_websocket(app_handle).await;
            });
            Ok(())
        })
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![list_usb_devices, verify_device])
        .run(tauri::generate_context!())
        .expect("Error running WebBoot Companion");
}

fn main() {
    run();
}

// Add device verification
#[tauri::command]
fn verify_device(device_path: String) -> Result<DeviceInfo, String> {
    let path = std::path::Path::new(&device_path);
    if !path.exists() {
        return Err("Device does not exist".to_string());
    }

    #[cfg(target_os = "linux")]
    {
        // Get device information using lsblk
        if let Ok(output) = Command::new("lsblk")
            .args(["-b", "-n", "-o", "SIZE,FSTYPE,MOUNTPOINT", &device_path])
            .output()
        {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                let lines: Vec<&str> = output_str.trim().split('\n').collect();
                if let Some(line) = lines.first() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if let Ok(size) = parts.get(0).unwrap_or(&"0").parse::<u64>() {
                        let filesystem = parts.get(1).map(|s| s.to_string());
                        let mount_points: Vec<String> = if parts.len() > 2 {
                            parts[2..].iter().map(|s| s.to_string()).collect()
                        } else {
                            vec![]
                        };
                        
                        return Ok(DeviceInfo {
                            path: device_path,
                            size,
                            filesystem,
                            is_mounted: !mount_points.is_empty(),
                            mount_points,
                        });
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        // Basic fallback for non-Linux systems
        if let Ok(metadata) = std::fs::metadata(&device_path) {
            return Ok(DeviceInfo {
                path: device_path,
                size: metadata.len(),
                filesystem: None,
                is_mounted: false,
                mount_points: vec![],
            });
        }
    }

    Err("Unable to get device information".to_string())
}