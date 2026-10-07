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
//! | [`connection`]  | Connection Manager: USB / Wi-Fi transports, pairing        |
//! | [`adb`]         | ADB integration: the single place that talks to `adb`      |
//! | [`mirroring`]   | Mirroring engine: session lifecycle for screen streaming   |
//! | [`input`]       | Input engine: touch / key / gesture injection              |
//! | [`video`]       | Native decode and GPU rendering of mirrored frames         |
//! | [`clipboard`]   | Clipboard synchronisation between desktop and device       |
//! | [`settings`]    | Persisted application settings                             |
//!
//! The bootstrap ships these boundaries plus the module registry; the
//! behaviours themselves are roadmap items (`docs/roadmap.md`).

pub mod adb;
pub mod clipboard;
pub mod connection;
pub mod device;
pub mod input;
pub mod mirroring;
pub mod settings;
pub mod video;

use serde::Serialize;

/// Bootstrap state of a core module.
///
/// Always `"planned"` for now. Unknown values are rendered generically by the
/// UI, so new states can be introduced per module without touching this crate.
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
    /// Current [`ModuleState`] (`"planned"` at bootstrap).
    pub state: ModuleState,
    /// What the module is responsible for.
    pub responsibility: &'static str,
}

/// Every native core boundary, in architecture order.
pub const MODULES: &[ModuleInfo] = &[
    ModuleInfo {
        id: "device",
        name: "Device Manager",
        state: "planned",
        responsibility: "Discover, identify and track connected Android devices.",
    },
    ModuleInfo {
        id: "connection",
        name: "Connection Manager",
        state: "planned",
        responsibility: "USB and Wi-Fi transports, wireless pairing and discovery.",
    },
    ModuleInfo {
        id: "adb",
        name: "ADB Integration",
        state: "planned",
        responsibility: "Single gateway to adb; no other module shells out.",
    },
    ModuleInfo {
        id: "mirroring",
        name: "Mirroring Engine",
        state: "planned",
        responsibility: "Session lifecycle for streaming a device screen.",
    },
    ModuleInfo {
        id: "input",
        name: "Input Engine",
        state: "planned",
        responsibility: "Inject touch, key and gesture events into the device.",
    },
    ModuleInfo {
        id: "video",
        name: "Video & Rendering",
        state: "planned",
        responsibility: "Native decode and GPU rendering of mirrored frames.",
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
