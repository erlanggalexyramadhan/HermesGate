//! Tauri command layer — the only surface the webview can reach.
//!
//! Commands stay thin: validate input, delegate to the native core
//! ([`crate::core`]), return plain serializable data. No shell, filesystem,
//! or device capability is exposed beyond what is listed here.

use crate::core;
use serde::Serialize;

/// Static application and runtime facts for the dashboard.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub platform: &'static str,
    pub arch: &'static str,
}

/// Application name, version and host platform.
#[tauri::command]
pub fn get_app_info() -> AppInfo {
    AppInfo {
        // Matches `productName` in tauri.conf.json.
        name: "HermesGate",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// Bootstrap status of every native core module.
#[tauri::command]
pub fn get_core_status() -> Vec<core::ModuleInfo> {
    core::MODULES.to_vec()
}

/// Broadcast on every device state change; payload is a [`DeviceSnapshot`].
pub const DEVICES_CHANGED: &str = "devices-changed";

/// Refresh the device list from the native core: attached Android devices,
/// their state and the availability of the ADB runtime.
///
/// Never fails — an unavailable `adb` is reported inside the snapshot.
#[tauri::command]
pub fn list_devices(
    manager: tauri::State<'_, std::sync::Arc<std::sync::Mutex<core::device::DeviceManager>>>,
) -> core::device::DeviceSnapshot {
    manager
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .refresh()
}

/// Poll `adb` and broadcast a fresh snapshot as [`DEVICES_CHANGED`] whenever
/// something actually changed (attachment, state, ADB availability).
///
/// ponytail: a 2 s poll instead of a long-lived `adb track-devices` child —
/// same result for a UI, no orphan process to reap, and no parse assumptions
/// about the tracker stream. Switch only if sub-second latency ever matters.
pub fn watch_devices(
    app: tauri::AppHandle,
    manager: std::sync::Arc<std::sync::Mutex<core::device::DeviceManager>>,
) {
    std::thread::spawn(move || {
        use tauri::Emitter;

        let mut last: Option<core::device::DeviceSnapshot> = None;
        loop {
            let snapshot = manager
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .refresh();
            if last.as_ref() != Some(&snapshot) {
                let _ = app.emit(DEVICES_CHANGED, &snapshot);
                last = Some(snapshot);
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

/// Broadcast on every mirror status change; payload is a
/// [`core::mirroring::MirrorStatus`].
pub const MIRROR_CHANGED: &str = "mirror-changed";

/// Start mirroring `serial` with the mode the Device page selected
/// (`sharp` | `balanced` | `smooth`); `None` keeps the engine's default knobs
/// (native resolution, 8 Mbit/s, uncapped fps). An unknown mode is rejected
/// instead of silently falling back, so the UI never claims a mode it did not
/// get.
///
/// `serial` is the serial of the transport the user chose
/// ([`core::device::DeviceEndpoint::serial`]): `bc058b2c` for USB,
/// `192.168.1.42:37000` for Wi-Fi. `adb` routes every pipeline step through it,
/// so the session runs over the selected transport rather than an arbitrary
/// endpoint of the same phone.
#[tauri::command]
pub fn start_mirror(serial: String, mode: Option<String>) -> Result<(), String> {
    let quality = match mode {
        Some(name) => core::mirroring::MirrorQuality::preset(&name)?,
        None => core::mirroring::MirrorQuality::default(),
    };
    core::mirroring::request_start(&serial, quality)
}

/// Stop the active mirror session. Idempotent: stopping while idle is a
/// no-op, matching the engine's [`core::mirroring::request_stop`].
#[tauri::command]
pub fn stop_mirror() -> Result<(), String> {
    core::mirroring::request_stop()
}

/// Current mirror session status (the idle default when nothing ever ran).
#[tauri::command]
pub fn mirror_status() -> core::mirroring::MirrorStatus {
    core::mirroring::poll_status()
}

/// Parse a `<ip>:<port>` address the webview sent for a wireless operation.
fn parse_address(address: &str) -> Result<std::net::SocketAddr, String> {
    address
        .trim()
        .parse()
        .map_err(|_| format!("invalid address '{address}': expected <ip>:<port>"))
}

/// Pair with a wireless device: `adb pair <address> <key>`.
///
/// `key` is the pairing code shown on the device (Developer options →
/// Wireless debugging → Pair device with pairing code). The resolved
/// [`core::connection::wifi::WifiManager`] is `None` when no ADB runtime
/// was found at startup.
#[tauri::command(async)]
pub fn wifi_pair(
    address: String,
    key: String,
    timeout_secs: Option<u64>,
    wifi: tauri::State<'_, Option<core::connection::wifi::WifiManager>>,
) -> Result<(), String> {
    let manager = wifi
        .as_ref()
        .ok_or_else(|| "ADB runtime unavailable; wireless pairing is disabled".to_string())?;
    let addr = parse_address(&address)?;
    manager.pair_device(addr, &key, timeout_secs.unwrap_or(120))?;
    Ok(())
}

/// Connect to an already-paired wireless device: `adb connect <address>`.
#[tauri::command(async)]
pub fn wifi_connect(
    address: String,
    wifi: tauri::State<'_, Option<core::connection::wifi::WifiManager>>,
) -> Result<(), String> {
    let manager = wifi
        .as_ref()
        .ok_or_else(|| "ADB runtime unavailable; wireless connections are disabled".to_string())?;
    let addr = parse_address(&address)?;
    manager.connect_device(addr)?;
    Ok(())
}

/// Disconnect a wireless device: `adb disconnect <address>`.
#[tauri::command(async)]
pub fn wifi_disconnect(
    address: String,
    wifi: tauri::State<'_, Option<core::connection::wifi::WifiManager>>,
) -> Result<(), String> {
    let manager = wifi
        .as_ref()
        .ok_or_else(|| "ADB runtime unavailable; wireless connections are disabled".to_string())?;
    let addr = parse_address(&address)?;
    manager.disconnect_device(addr)?;
    Ok(())
}

/// Poll the mirror engine and broadcast a fresh snapshot as
/// [`MIRROR_CHANGED`] whenever something actually changed (phase, reason,
/// frame size, serial, or the 1 Hz pipeline counters the session publishes).
///
/// ponytail: a 1 s poll instead of hooking every engine transition — the
/// engine already keeps a plain `MirrorStatus`, and a UI that reacts within
/// a second needs no event plumbing inside the session lifecycle. Switch to
/// direct emits only if sub-second latency ever matters.
pub fn watch_mirror(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        use tauri::Emitter;

        let mut last: Option<core::mirroring::MirrorStatus> = None;
        loop {
            let snapshot = core::mirroring::poll_status();
            if last.as_ref() != Some(&snapshot) {
                let _ = app.emit(MIRROR_CHANGED, &snapshot);
                last = Some(snapshot);
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}

/// Send one navigation key press to the device over the running session's
/// control channel.
///
/// `key` is the name the session panel uses. Fails rather than silently doing
/// nothing when no session is forwarding input.
#[tauri::command]
pub fn mirror_key(key: String) -> Result<(), String> {
    let keycode = match key.as_str() {
        "back" => core::input::KEYCODE_BACK,
        "home" => core::input::KEYCODE_HOME,
        "recents" => core::input::KEYCODE_APP_SWITCH,
        other => return Err(format!("unknown navigation key '{other}'")),
    };
    let Some(controller) = core::input::active() else {
        return Err("no mirror session is forwarding input".to_string());
    };
    controller.key(keycode);
    Ok(())
}
