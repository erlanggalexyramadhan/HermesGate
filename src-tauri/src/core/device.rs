//! Device Manager boundary.
//!
//! Owns device identity and lifecycle: what devices HermesGate knows about
//! and whether they are reachable. Connectivity itself comes from
//! [`crate::core::connection`] (`Transport`), metadata from [`crate::core::adb`].

use super::adb;
use super::connection::Transport;
use serde::Serialize;
use std::collections::HashMap;

/// Lifecycle state of a known device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceState {
    /// Attached and authorised for debugging.
    Online,
    /// Attached, but this computer is not authorised on the device.
    Unauthorized,
    /// Attached but not usable (`adb` reports `offline`, `recovery`, …).
    Offline,
    /// Seen this session, currently not attached.
    Disconnected,
}

impl DeviceState {
    /// Map an `adb devices` state word onto a HermesGate state.
    fn from_adb(state: &str) -> Self {
        match state {
            "device" => DeviceState::Online,
            "unauthorized" | "authorizing" => DeviceState::Unauthorized,
            _ => DeviceState::Offline,
        }
    }

    /// List order: reachable first, detached last.
    fn rank(self) -> u8 {
        match self {
            DeviceState::Online => 0,
            DeviceState::Unauthorized => 1,
            DeviceState::Offline => 2,
            DeviceState::Disconnected => 3,
        }
    }
}

/// One device as the command/event interface exposes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub serial: String,
    pub state: DeviceState,
    /// How HermesGate reaches this device: `usb` or `wifi` (Wi-Fi serials are
    /// `ip:port`), so both transports flow through the same connection layer.
    pub transport: Transport,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub android_version: Option<String>,
}

/// Availability of the ADB runtime, reported with every device list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbStatus {
    pub available: bool,
    /// Where the resolved `adb` came from: `bundled`, `path`, `sdk` or `env`.
    pub source: Option<String>,
    pub path: Option<String>,
    pub error: Option<String>,
}

/// What one refresh produced: devices plus the state of the ADB runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshot {
    pub devices: Vec<DeviceInfo>,
    pub adb: AdbStatus,
}

/// Device identity and lifecycle for the whole session.
#[derive(Default)]
pub struct DeviceManager {
    /// Every device seen this session, keyed by serial. Devices stay here as
    /// `Disconnected` instead of vanishing, so the UI can show what happened.
    known: HashMap<String, DeviceInfo>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Refresh all known devices from `adb` and report the current picture.
    ///
    /// Never fails: when `adb` is missing or a call errors the last known
    /// devices are returned together with an [`AdbStatus`] carrying the
    /// problem, so the UI can show it instead of breaking.
    pub fn refresh(&mut self) -> DeviceSnapshot {
        let adb = match adb::resolve() {
            Ok(adb) => adb,
            Err(error) => {
                return self.snapshot(AdbStatus {
                    available: false,
                    source: None,
                    path: None,
                    error: Some(error),
                })
            }
        };
        let status = AdbStatus {
            available: true,
            source: Some(adb.source.as_str().to_string()),
            path: Some(adb.path.display().to_string()),
            error: None,
        };
        let raw = match adb::list_devices(&adb.path) {
            Ok(raw) => raw,
            Err(error) => {
                return self.snapshot(AdbStatus {
                    error: Some(error),
                    ..status
                })
            }
        };

        // Anything not attached right now is disconnected — until it shows up
        // again below.
        for device in self.known.values_mut() {
            device.state = DeviceState::Disconnected;
        }
        for entry in raw {
            let serial = entry.serial;
            let transport = Transport::for_serial(&serial);
            let device = self
                .known
                .entry(serial.clone())
                .or_insert_with(|| DeviceInfo {
                    serial,
                    state: DeviceState::Offline,
                    transport,
                    manufacturer: None,
                    model: None,
                    android_version: None,
                });
            device.state = DeviceState::from_adb(&entry.state);
            device.transport = transport;
            if device.model.is_none() {
                device.model = entry.model;
            }

            // Identify online devices once; the values then come from the
            // cache instead of another `getprop` round trip.
            let identified = device.manufacturer.is_some() && device.android_version.is_some();
            if device.state == DeviceState::Online && !identified {
                let serial = device.serial.clone();
                if let Ok(props) = adb::device_props(&adb.path, &serial) {
                    if let Some(device) = self.known.get_mut(&serial) {
                        if props.manufacturer.is_some() {
                            device.manufacturer = props.manufacturer;
                        }
                        if props.model.is_some() {
                            device.model = props.model;
                        }
                        if props.android_version.is_some() {
                            device.android_version = props.android_version;
                        }
                    }
                }
            }
        }
        self.snapshot(status)
    }

    fn snapshot(&self, adb: AdbStatus) -> DeviceSnapshot {
        let mut devices: Vec<DeviceInfo> = self.known.values().cloned().collect();
        devices.sort_by(|a, b| {
            a.state
                .rank()
                .cmp(&b.state.rank())
                .then(a.serial.cmp(&b.serial))
        });
        DeviceSnapshot { devices, adb }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_adb_states() {
        assert_eq!(DeviceState::from_adb("device"), DeviceState::Online);
        assert_eq!(
            DeviceState::from_adb("unauthorized"),
            DeviceState::Unauthorized
        );
        assert_eq!(
            DeviceState::from_adb("authorizing"),
            DeviceState::Unauthorized
        );
        assert_eq!(DeviceState::from_adb("offline"), DeviceState::Offline);
        assert_eq!(DeviceState::from_adb("recovery"), DeviceState::Offline);
    }
}
