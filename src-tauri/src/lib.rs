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
            commands::get_core_status,
            commands::list_devices
        ])
        .setup(|app| {
            use tauri::Manager;

            // One shared Device Manager: the polling watcher and the
            // `list_devices` command refresh the same state.
            let manager =
                std::sync::Arc::new(std::sync::Mutex::new(core::device::DeviceManager::new()));
            app.manage(manager.clone());
            commands::watch_devices(app.handle().clone(), manager);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("HermesGate failed to start");
}
