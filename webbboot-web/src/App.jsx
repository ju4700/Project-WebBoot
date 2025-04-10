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
          <li><a href="/downloads/webbboot-companion-linux.AppImage" download>Linux (.deb)</a></li>
          <li><a href="/downloads/webbboot-companion-linux.AppImage" download>Linux (.rpm)</a></li>
          <li><a href="/downloads/webbboot-companion-windows.exe" download>Windows (.exe)</a></li>
          <li><a href="/downloads/webbboot-companion-macos.dmg" download>macOS (.dmg)</a></li>
        </ul>
        <p>After downloading, run the app, then refresh this page to start.</p>
        <p>Made by <a href='https://github.com/ju4700'>ju4700</a></p>
      </section>
    </div>
  );
}

export default App;