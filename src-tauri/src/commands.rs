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
