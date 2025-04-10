import React from 'react';
import './styles.css';
import BootForm from './components/BootForm';

function App() {
  return (
    <div className="App">
      <BootForm />
      <section>
        <h2>Get WebBoot Companion</h2>
        <p>To use WebBoot, download and run the companion app for your platform:</p>
        <ul>
          <li><a className="download-btn" href="/downloads/webbboot-companion-linux.deb" download>Linux (DEB)</a></li>
          <li><a className="download-btn" href="/downloads/webbboot-companion-linux.rpm" download>Linux (RPM)</a></li>
          <li><a className="download-btn" href="/downloads/webbboot-companion-windows.exe" download>Windows (EXE)</a></li>
          <li><a className="download-btn" href="/downloads/webbboot-companion-macos.dmg" download>macOS (DMG)</a></li>
        </ul>
        <p>After downloading, install/run the app, then refresh this page to start.</p>
        <p>Made by <a className="link" href="https://github.com/ju4700">ju4700</a></p>
      </section>
    </div>
  );
}

export default App;