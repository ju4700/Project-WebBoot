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

  useEffect(() => {
    const websocket = new WebSocket("ws://localhost:8080");
    websocket.onopen = () => setStatus("Connected to WebBoot Companion");
    websocket.onmessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.devices) {
        setUsbDevices(data.devices);
        setSelectedDevice(data.devices[0] || "");
      }
      if (data.status) setStatus(data.status);
      if (data.progress) setProgress(data.progress);
    };
    websocket.onerror = () =>
      setStatus(
        <span>
          Companion app not running.{" "}
          <a href="https://github.com/ju4700/webbboot/releases" className="link">
            Download WebBoot Companion
          </a>
        </span>
      );
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
      iso: iso ? iso.name : null,
      filesystem: fileSystem,
      scheme,
      device: selectedDevice,
    };
    ws.send(JSON.stringify(job));
    setStatus(`Starting ${action}...`);
    setProgress(0);
  };

  return (
    <div className="boot-form">
      <h1>WebBoot</h1>
      <input
        type="file"
        accept=".iso"
        onChange={handleIsoUpload}
        className="input-file"
      />
      <select
        value={fileSystem}
        onChange={(e) => setFileSystem(e.target.value)}
        className="select"
      >
        <option value="FAT32">FAT32</option>
        <option value="NTFS">NTFS</option>
        <option value="exFAT">exFAT</option>
      </select>
      <select
        value={scheme}
        onChange={(e) => setScheme(e.target.value)}
        className="select"
      >
        <option value="MBR">MBR</option>
        <option value="GPT">GPT</option>
      </select>
      <select
        value={selectedDevice}
        onChange={(e) => setSelectedDevice(e.target.value)}
        className="select"
      >
        <option value="">Select USB Device</option>
        {usbDevices.map((dev, i) => (
          <option key={i} value={dev}>
            {dev}
          </option>
        ))}
      </select>
      <div className="button-group">
        <button onClick={() => sendJob("create")} className="button create-btn">
          Create Bootable USB
        </button>
        <button onClick={() => sendJob("restore")} className="button restore-btn">
          Restore USB
        </button>
      </div>
      <div className="progress-bar">
        <div className="progress" style={{ width: `${progress}%` }} />
      </div>
      <p className="status">{status}</p>
    </div>
  );
}

export default BootForm;