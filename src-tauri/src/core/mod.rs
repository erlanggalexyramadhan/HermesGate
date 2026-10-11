//! HermesGate native core.
//!
//! Everything that touches the operating system, a device, or pixels lives
//! behind this boundary. The webview gets no shell, filesystem, or device
//! capability: it can only call the Tauri commands in [`crate::commands`],
//! which validate and delegate here.
//!
//! Boundaries (see `docs/architecture.md`):
//!
//! | Module          | Responsibility                                            |
//! |-----------------|-----------------------------------------------------------|
//! | [`device`]      | Device Manager: discovery, identity and state of devices  |
//! | [`audio`]       | Audio engine: the device's playback, played on the desktop|
//! | [`connection`]  | Connection Manager: USB / Wi-Fi transports, pairing        |
//! | [`adb`]         | ADB integration: the single place that talks to `adb`      |
//! | [`mirroring`]   | Mirroring engine: session lifecycle for screen streaming   |
//! | [`input`]       | Input engine: touch / key / gesture injection              |
//! | [`video`]       | Native decode and GPU rendering of mirrored frames         |
//! | [`clipboard`]   | Clipboard synchronisation between desktop and device       |
//! | [`settings`]    | Persisted application settings                             |
//!
//! The registry reports each module's real state: `device`, `adb` and `audio`
//! are complete for V1 (device detection, the single `adb` gateway, and playing
//! the device's audio on the desktop), `connection`, `mirroring`, `input` and
//! `video` are partial (verified live, with named gaps), and `clipboard` and
//! `settings` are roadmap items (`docs/roadmap.md`).

pub mod adb;
pub mod audio;
pub mod clipboard;
pub mod connection;
pub mod device;
#[cfg(windows)]
pub mod gpu;
pub mod h264;
pub mod input;
pub mod mirroring;
pub mod pixels;
pub mod settings;
pub mod video;

use serde::Serialize;

/// State of a core module.
///
/// `"planned"`, `"partial"` or `"ready"`. Unknown values are rendered
/// generically by the UI, so new states can be introduced per module without
/// touching this crate.
pub type ModuleState = &'static str;

/// One row of the native core registry, surfaced to the UI by
/// [`crate::commands::get_core_status`].
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleInfo {
    /// Stable identifier, e.g. `"connection"`.
    pub id: &'static str,
    /// Human-readable module name.
    pub name: &'static str,
    /// Current [`ModuleState`] (`"planned"`, `"partial"` or `"ready"`).
    pub state: ModuleState,
    /// What the module is responsible for.
    pub responsibility: &'static str,
}

/// Every native core boundary, in architecture order.
pub const MODULES: &[ModuleInfo] = &[
    ModuleInfo {
        id: "device",
        name: "Device Manager",
        state: "ready",
        responsibility: "Discover, identify and track connected Android devices.",
    },
    ModuleInfo {
        id: "connection",
        name: "Connection Manager",
        state: "partial",
        responsibility: "USB and Wi-Fi transports, wireless pairing and discovery.",
    },
    ModuleInfo {
        id: "adb",
        name: "ADB Integration",
        state: "ready",
        responsibility: "Single gateway to adb; no other module shells out.",
    },
    ModuleInfo {
        id: "mirroring",
        name: "Mirroring Engine",
        state: "partial",
        responsibility: "Session lifecycle for streaming a device screen.",
    },
    ModuleInfo {
        id: "input",
        name: "Input Engine",
        state: "partial",
        responsibility: "Inject touch, key and gesture events into the device.",
    },
    ModuleInfo {
        id: "video",
        name: "Video & Rendering",
        state: "partial",
        responsibility: "Native decode and GPU rendering of mirrored frames.",
    },
    ModuleInfo {
        id: "audio",
        name: "Audio Engine",
        state: "ready",
        responsibility: "Play the device's audio stream on the desktop output.",
    },
    ModuleInfo {
        id: "clipboard",
        name: "Clipboard",
        state: "planned",
        responsibility: "Keep the desktop and device clipboards in sync.",
    },
    ModuleInfo {
        id: "settings",
        name: "Settings",
        state: "planned",
        responsibility: "Persisted application preferences.",
    },
];
