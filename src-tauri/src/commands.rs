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
