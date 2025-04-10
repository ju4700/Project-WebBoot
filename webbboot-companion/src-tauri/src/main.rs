#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rusb::{devices};
use serde::{Serialize, Deserialize};
use std::process::Command;
use std::path::Path;
use log::{info, error};
use simplelog::{TermLogger, Config, LevelFilter};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use futures_util::{SinkExt, StreamExt};

#[derive(Serialize, Deserialize, Debug)]
struct Job {
    action: String,
    iso: Option<String>,
    filesystem: String,
    scheme: String,
    device: String,
}

#[tauri::command]
fn list_usb_devices() -> Vec<String> {
    match devices() {
        Ok(dev_list) => dev_list
            .iter()
            .filter_map(|dev| {
                match dev.device_descriptor() {
                    Ok(desc) => {
                        // Open the device to get its configuration
                        match dev.open() {
                            Ok(handle) => {
                                // Get the active configuration
                                if let Ok(config) = handle.active_configuration() {
                                    // Get the configuration descriptor
                                    if let Ok(config_desc) = dev.config_descriptor(config - 1) {
                                        // Check interfaces for mass storage class (0x08)
                                        let is_mass_storage = config_desc
                                            .interfaces()
                                            .flat_map(|interface| interface.descriptors())
                                            .any(|desc| desc.class_code() == 0x08); // Mass Storage Class
                                        if is_mass_storage {
                                            Some(format!("USB {:04x}:{:04x}", desc.vendor_id(), desc.product_id()))
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

async fn execute_job(job: Job, write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, tokio_tungstenite::tungstenite::Message>) {
    info!("Received job: {:?}", job);
    write.send(tokio_tungstenite::tungstenite::Message::Text(
        r#"{"status": "Formatting...", "progress": 33}"#.into()
    )).await.unwrap();

    #[cfg(target_os = "linux")]
    let format_result = Command::new("sudo")
        .args(["mkfs", &format!("-t{}", job.filesystem.to_lowercase()), &job.device])
        .output();

    #[cfg(target_os = "linux")]
    match format_result {
        Ok(output) if output.status.success() => info!("Formatted {}", job.device),
        _ => {
            error!("Format failed for {}", job.device);
            write.send(tokio_tungstenite::tungstenite::Message::Text(
                r#"{"status": "Format failed (run with sudo?)", "progress": 0}"#.into()
            )).await.unwrap();
            return;
        }
    }

    if job.action == "create" {
        write.send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{"status": "Writing ISO...", "progress": 66}"#.into()
        )).await.unwrap();

        let iso_path = job.iso.as_ref().unwrap();
        if !Path::new(iso_path).exists() {
            error!("ISO file not found: {}", iso_path);
            write.send(tokio_tungstenite::tungstenite::Message::Text(
                r#"{"status": "ISO file not found", "progress": 0}"#.into()
            )).await.unwrap();
            return;
        }

        #[cfg(target_os = "linux")]
        let write_result = Command::new("sudo")
            .args(["dd", &format!("if={}", iso_path), &format!("of={}", job.device), "bs=4M", "status=progress"])
            .output();

        #[cfg(target_os = "linux")]
        match write_result {
            Ok(output) if output.status.success() => info!("Wrote ISO to {}", job.device),
            _ => {
                error!("ISO write failed for {}", job.device);
                write.send(tokio_tungstenite::tungstenite::Message::Text(
                    r#"{"status": "ISO write failed (run with sudo?)", "progress": 0}"#.into()
                )).await.unwrap();
                return;
            }
        }
    }

    write.send(tokio_tungstenite::tungstenite::Message::Text(
        r#"{"status": "Done", "progress": 100}"#.into()
    )).await.unwrap();
}

async fn handle_websocket(_app: tauri::AppHandle) {
    TermLogger::init(
        LevelFilter::Info, 
        Config::default(), 
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto
    ).unwrap();
    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    info!("WebSocket server started on ws://localhost:8080");

    while let Ok((stream, _)) = listener.accept().await {
        let ws_stream = accept_async(stream).await.unwrap();
        let (mut write, mut read) = ws_stream.split();

        let devices = list_usb_devices();
        let msg = serde_json::to_string(&serde_json::json!({"devices": devices})).unwrap();
        write.send(tokio_tungstenite::tungstenite::Message::Text(msg)).await.unwrap();

        while let Some(Ok(msg)) = read.next().await {
            if let Ok(job) = serde_json::from_str::<Job>(&msg.to_string()) {
                execute_job(job, &mut write).await;
            }
        }
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
        .invoke_handler(tauri::generate_handler![list_usb_devices])
        .run(tauri::generate_context!())
        .expect("Error running WebBoot Companion");
}

fn main() {
    run();
}