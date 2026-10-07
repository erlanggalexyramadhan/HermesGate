//! HermesGate desktop entry point.
//!
//! Layering: webview (React) → Tauri commands ([`commands`]) → native core
//! ([`core`]). The webview is deliberately limited to `core:default`
//! capabilities; every privileged operation is a command implemented in Rust.

pub mod commands;
pub mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_core_status
        ])
        .run(tauri::generate_context!())
        .expect("HermesGate failed to start");
}
