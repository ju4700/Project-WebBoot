import React, { useState, useEffect } from "react";

function BootForm() {
  const [iso, setIso] = useState(null);
  const [fileSystem, setFileSystem] = useState("FAT32");
  const [scheme, setScheme] = useState("MBR");
  const [usbDevices, setUsbDevices] = useState([]);
  const [selectedDevice, setSelectedDevice] = useState("");
  const [status, setStatus] = useState("Download WebBoot Companion to start");
  const [progress, setProgress] = useState(0);
  const [ws, setWs] = useState(null);
  const [currentOperation, setCurrentOperation] = useState("");
  const [isVerifying, setIsVerifying] = useState(false);
  const [deviceInfo, setDeviceInfo] = useState(null);

  useEffect(() => {
    const websocket = new WebSocket("ws://localhost:8080");
    websocket.onopen = () => setStatus("Connected to WebBoot Companion");
    websocket.onmessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.devices) {
        setUsbDevices(data.devices);
        setSelectedDevice(data.devices[0]?.id || "");
      }
      if (data.status) setStatus(data.status);
      if (data.progress !== undefined) setProgress(data.progress);
      if (data.current_operation) setCurrentOperation(data.current_operation);
    };
    websocket.onerror = () =>
      setStatus("Companion app not running. Download WebBoot Companion");
    websocket.onclose = () => setStatus("Companion app disconnected");
    setWs(websocket);
    return () => websocket.close();
  }, []);

  const handleIsoUpload = (e) => setIso(e.target.files[0]);

  const sendJob = (action) => {
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      setStatus("Companion app not connected");
      return;
    }
    if (action === "create" && !iso) {
      setStatus("Please upload an ISO file");
      return;
    }
    if (!selectedDevice) {
      setStatus("Please select a USB device");
      return;
    }
    const job = {
      action,
      iso: iso ? iso.path || iso.name : null,
      filesystem: fileSystem,
      scheme,
      device: selectedDevice,
    };
    ws.send(JSON.stringify(job));
    setStatus(`Starting ${action}...`);
    setProgress(0);
  };

  const verifyDevice = async (devicePath) => {
    if (!devicePath) return;

    setIsVerifying(true);
    try {
      // This would call the verify_device Tauri command
      // For now, we'll simulate verification
      await new Promise((resolve) => setTimeout(resolve, 1000));
      setDeviceInfo({
        path: devicePath,
        size: "8 GB",
        filesystem: "FAT32",
        is_mounted: false,
      });
    } catch (error) {
      console.error("Device verification failed:", error);
    } finally {
      setIsVerifying(false);
    }
  };

  const formatSize = (bytes) => {
    if (!bytes) return "";
    const gb = bytes / (1024 * 1024 * 1024);
    return gb >= 1
      ? `${gb.toFixed(1)} GB`
      : `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
  };

  return (
    <div className="boot-form">
      <h1>WebBoot</h1>
      <div className="form-section">
        <label>Select ISO File:</label>
        <input
          type="file"
          accept=".iso"
          onChange={handleIsoUpload}
          className="input-file"
        />
        {iso && <div className="file-info">Selected: {iso.name}</div>}
      </div>

      <div className="form-section">
        <label>File System:</label>
        <select
          value={fileSystem}
          onChange={(e) => setFileSystem(e.target.value)}
          className="select"
        >
          <option value="FAT32">FAT32</option>
          <option value="NTFS">NTFS</option>
          <option value="exFAT">exFAT</option>
        </select>
      </div>

      <div className="form-section">
        <label>Partition Scheme:</label>
        <select
          value={scheme}
          onChange={(e) => setScheme(e.target.value)}
          className="select"
        >
          <option value="MBR">MBR (Legacy BIOS)</option>
          <option value="GPT">GPT (UEFI)</option>
        </select>
      </div>

      <div className="form-section">
        <label>USB Device:</label>
        <select
          value={selectedDevice}
          onChange={(e) => {
            setSelectedDevice(e.target.value);
            verifyDevice(e.target.value);
          }}
          className="select"
        >
          <option value="">Select USB Device</option>
          {usbDevices.map((dev, i) => (
            <option key={i} value={dev.id}>
              {dev.name}{" "}
              {dev.size ? `(${formatSize(dev.size)})` : ""}
            </option>
          ))}
        </select>
        {isVerifying && (
          <div className="verification-status">Verifying device...</div>
        )}
        {deviceInfo && (
          <div className="device-info">
            <p>Path: {deviceInfo.path}</p>
            <p>Size: {deviceInfo.size}</p>
            <p>Filesystem: {deviceInfo.filesystem || "Unknown"}</p>
            <p>Status: {deviceInfo.is_mounted ? "Mounted" : "Unmounted"}</p>
          </div>
        )}
      </div>

      <div className="button-group">
        <button
          onClick={() => sendJob("create")}
          className="button create-btn"
          disabled={!iso || !selectedDevice || isVerifying}
        >
          Create Bootable USB
        </button>
        <button
          onClick={() => sendJob("restore")}
          className="button restore-btn"
          disabled={!selectedDevice || isVerifying}
        >
          Restore USB
        </button>
      </div>

      <div className="progress-section">
        <div className="progress-bar">
          <div className="progress" style={{ width: `${progress}%` }} />
        </div>
        <div className="progress-text">{progress}%</div>
        {currentOperation && (
          <div className="operation-status">Current: {currentOperation}</div>
        )}
      </div>

      <div className="status-section">
        <p className="status">{status}</p>
      </div>
    </div>
  );
}

export default BootForm;
