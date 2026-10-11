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

    /// List order: reachable first.
    fn rank(self) -> u8 {
        match self {
            DeviceState::Online => 0,
            DeviceState::Unauthorized => 1,
            DeviceState::Offline => 2,
        }
    }
}

/// One way to reach a device: the transport, and the ADB serial that addresses
/// it. This is what a mirror session is started with, so the user's transport
/// choice is the one that reaches `adb`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEndpoint {
    pub transport: Transport,
    pub serial: String,
}

/// One *physical* device as the command/event interface exposes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// The most stable serial the device answers to (see [`serial_rank`]),
    /// used to identify the card. Start mirroring with an endpoint serial, not
    /// this one, when a specific transport was chosen.
    pub serial: String,
    pub state: DeviceState,
    /// Every transport the device is reachable on right now, USB first, one
    /// entry each. A phone that is plugged in *and* paired reports `usb` and
    /// `wifi` — on one card, because it is one device.
    pub endpoints: Vec<DeviceEndpoint>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub android_version: Option<String>,
    /// Identity of the physical device (`ro.serialno`), the grouping key.
    /// Internal: the UI renders one card per identity and never needs the key.
    #[serde(skip)]
    identity: String,
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
    /// Metadata per ADB serial, read once per session: `getprop` is a shell
    /// round trip and the values do not change while a device stays attached.
    props: HashMap<String, adb::DeviceProps>,
    /// Last picture, keyed by [`DeviceInfo::identity`]. Rebuilt from scratch on
    /// every refresh: a device that is no longer reachable on any transport
    /// stops being reported.
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

        // Identify attached devices once per session, then group: `adb` lists
        // the same phone under several serials, and the identity that ties them
        // together is one only the device itself can report. Metadata is read
        // over the serial whose transport is most likely to answer first, and
        // over any other serial of a device that is still unidentified.
        for entry in &raw {
            if entry.state == "device" && !self.props.contains_key(&entry.serial) {
                if let Ok(props) = adb::device_props(&adb.path, &entry.serial) {
                    self.props.insert(entry.serial.clone(), props);
                }
            }
        }

        self.known = group_devices(&raw, &self.props);
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

/// Group the serials of one `adb devices` read by the physical device behind
/// them, so a phone that is reachable several ways is one [`DeviceInfo`].
fn group_devices(
    raw: &[adb::RawDevice],
    props: &HashMap<String, adb::DeviceProps>,
) -> HashMap<String, DeviceInfo> {
    let mut groups: HashMap<String, DeviceInfo> = HashMap::new();
    for entry in raw {
        let metadata = props.get(&entry.serial);
        let identity = identity(&entry.serial, metadata);
        let state = DeviceState::from_adb(&entry.state);
        let transport = Transport::for_serial(&entry.serial);
        let group = groups
            .entry(identity.clone())
            .or_insert_with(|| DeviceInfo {
                serial: entry.serial.clone(),
                state,
                endpoints: Vec::new(),
                manufacturer: None,
                model: entry.model.clone(),
                android_version: None,
                identity,
            });
        // The card is addressed by the most stable serial any of its transports
        // has, and reports the best state they reached: a phone that is `device`
        // over Wi-Fi is online even while its USB alias is `offline`.
        if serial_rank(&entry.serial) < serial_rank(&group.serial) {
            group.serial = entry.serial.clone();
        }
        if state.rank() < group.state.rank() {
            group.state = state;
        }
        add_endpoint(&mut group.endpoints, transport, &entry.serial);
        if group.model.is_none() {
            group.model = entry.model.clone();
        }
        if let Some(props) = metadata {
            fill(&mut group.manufacturer, &props.manufacturer);
            fill(&mut group.model, &props.model);
            fill(&mut group.android_version, &props.android_version);
        }
    }
    for device in groups.values_mut() {
        device
            .endpoints
            .sort_by_key(|endpoint| transport_rank(endpoint.transport));
    }
    groups
}

/// Keep one endpoint per transport, addressed by the most stable serial that
/// transport has: `adb` offers the USB link both as a hardware serial and as an
/// mDNS alias, and only one of them belongs on the card.
fn add_endpoint(endpoints: &mut Vec<DeviceEndpoint>, transport: Transport, serial: &str) {
    match endpoints
        .iter_mut()
        .find(|endpoint| endpoint.transport == transport)
    {
        Some(existing) => {
            if serial_rank(serial) < serial_rank(&existing.serial) {
                existing.serial = serial.to_string();
            }
        }
        None => endpoints.push(DeviceEndpoint {
            transport,
            serial: serial.to_string(),
        }),
    }
}

/// Keep what is already known: several serials of one device report the same
/// fields, and any of them may be the one that answered `getprop`.
fn fill(slot: &mut Option<String>, value: &Option<String>) {
    if slot.is_none() {
        *slot = value.clone();
    }
}

/// The physical device behind one ADB serial: what the device calls itself
/// (`ro.serialno`), else the serial embedded in an mDNS alias, else the serial.
fn identity(serial: &str, props: Option<&adb::DeviceProps>) -> String {
    if let Some(serialno) = props
        .and_then(|props| props.serialno.as_deref())
        .filter(|serialno| !serialno.is_empty())
    {
        return serialno.to_string();
    }
    mdns_serial(serial).unwrap_or_else(|| serial.to_string())
}

/// `<serial>` inside an mDNS service name
/// (`adb-<serial>-XXXXXX._adb-tls-connect._tcp`), the alias USB debugging
/// advertises next to the plain serial.
fn mdns_serial(serial: &str) -> Option<String> {
    let rest = serial.strip_prefix("adb-")?;
    // First `-`, not last: the suffix itself contains hyphens
    // (`8STQEK._adb-tls-connect._tcp`).
    let (serial, suffix) = rest.split_once('-')?;
    (!serial.is_empty() && suffix.ends_with("._adb-tls-connect._tcp")).then(|| serial.to_string())
}

/// Preference order among the serials one device answers to: the plain hardware
/// serial over USB (most stable, and what `adb -s` takes on every platform),
/// then its mDNS alias, then `ip:port`.
fn serial_rank(serial: &str) -> u8 {
    if serial.contains(':') {
        2
    } else if serial.starts_with("adb-") {
        1
    } else {
        0
    }
}

/// Card order of the transports a device is reachable on: USB first.
fn transport_rank(transport: Transport) -> u8 {
    match transport {
        Transport::Usb => 0,
        Transport::Wifi => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(serialno: &str, model: &str) -> adb::DeviceProps {
        adb::DeviceProps {
            serialno: Some(serialno.to_string()),
            model: Some(model.to_string()),
            ..Default::default()
        }
    }

    fn raw(serial: &str) -> adb::RawDevice {
        adb::RawDevice {
            serial: serial.to_string(),
            state: "device".to_string(),
            model: Some("CPH2773".to_string()),
        }
    }

    fn usb(serial: &str) -> DeviceEndpoint {
        DeviceEndpoint {
            transport: Transport::Usb,
            serial: serial.to_string(),
        }
    }

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

    #[test]
    fn reads_the_serial_out_of_an_mdns_alias() {
        assert_eq!(
            mdns_serial("adb-bc058b2c-8STQEK._adb-tls-connect._tcp").as_deref(),
            Some("bc058b2c")
        );
        assert_eq!(mdns_serial("bc058b2c"), None);
        assert_eq!(mdns_serial("adb-bc058b2c"), None);
        assert_eq!(mdns_serial("adb--8STQEK._adb-tls-connect._tcp"), None);
    }

    /// Verbatim from the bench: `adb devices -l` listed one phone three times —
    /// hardware serial, mDNS alias and wireless address. One card, two
    /// transports, each addressed by its own most stable serial.
    #[test]
    fn one_phone_reachable_three_ways_is_one_device() {
        let raw = vec![
            raw("bc058b2c"),
            raw("192.168.18.7:39583"),
            raw("adb-bc058b2c-8STQEK._adb-tls-connect._tcp"),
        ];
        let props: HashMap<String, adb::DeviceProps> = raw
            .iter()
            .map(|entry| (entry.serial.clone(), props("bc058b2c", "CPH2773")))
            .collect();

        let groups = group_devices(&raw, &props);

        assert_eq!(groups.len(), 1);
        let device = groups.values().next().unwrap();
        assert_eq!(device.serial, "bc058b2c");
        assert_eq!(
            device.endpoints,
            vec![
                usb("bc058b2c"),
                DeviceEndpoint {
                    transport: Transport::Wifi,
                    serial: "192.168.18.7:39583".to_string(),
                }
            ]
        );
        assert_eq!(device.state, DeviceState::Online);
        assert_eq!(device.model.as_deref(), Some("CPH2773"));
    }

    /// Before `getprop` answers, the mDNS alias alone already identifies the
    /// USB link, so the duplicate card never appears.
    #[test]
    fn an_mdns_alias_groups_with_its_serial_without_props() {
        let raw = vec![
            raw("adb-bc058b2c-8STQEK._adb-tls-connect._tcp"),
            raw("bc058b2c"),
        ];
        let groups = group_devices(&raw, &HashMap::new());

        assert_eq!(groups.len(), 1);
        let device = groups.values().next().unwrap();
        assert_eq!(device.serial, "bc058b2c");
        assert_eq!(device.endpoints, vec![usb("bc058b2c")]);
    }

    /// Two different phones stay two devices even when they are the same model.
    #[test]
    fn two_phones_stay_two_devices() {
        let raw = vec![raw("R5CT30ABCXY"), raw("bc058b2c")];
        let props: HashMap<String, adb::DeviceProps> = [
            ("R5CT30ABCXY".to_string(), props("R5CT30ABCXY", "CPH2773")),
            ("bc058b2c".to_string(), props("bc058b2c", "CPH2773")),
        ]
        .into_iter()
        .collect();

        assert_eq!(group_devices(&raw, &props).len(), 2);
    }

    /// The best state of a group wins: online over Wi-Fi while the USB alias is
    /// offline is still one online device, still on one card.
    #[test]
    fn the_group_reports_the_state_that_can_be_used() {
        let wifi = "192.168.18.7:39583";
        let raw = vec![
            adb::RawDevice {
                serial: "bc058b2c".to_string(),
                state: "offline".to_string(),
                model: None,
            },
            raw(wifi),
        ];
        let props: HashMap<String, adb::DeviceProps> = [
            ("bc058b2c".to_string(), props("bc058b2c", "CPH2773")),
            (wifi.to_string(), props("bc058b2c", "CPH2773")),
        ]
        .into_iter()
        .collect();

        let groups = group_devices(&raw, &props);
        assert_eq!(groups.len(), 1);
        let device = groups.values().next().unwrap();
        assert_eq!(device.state, DeviceState::Online);
        assert_eq!(device.endpoints.len(), 2);
    }

    /// USB unplugged, Wi-Fi still up: the same card, Wi-Fi still selectable.
    #[test]
    fn losing_one_transport_keeps_the_card_and_the_other_transport() {
        let wifi = "192.168.18.7:39583";
        let props: HashMap<String, adb::DeviceProps> = [
            ("bc058b2c".to_string(), props("bc058b2c", "CPH2773")),
            (wifi.to_string(), props("bc058b2c", "CPH2773")),
        ]
        .into_iter()
        .collect();

        let both = group_devices(&[raw("bc058b2c"), raw(wifi)], &props);
        let after = group_devices(&[raw(wifi)], &props);

        assert_eq!(both.len(), 1);
        assert_eq!(after.len(), 1);
        let before = both.values().next().unwrap();
        let device = after.values().next().unwrap();
        assert_eq!(device.identity, before.identity);
        assert_eq!(
            device.endpoints,
            vec![DeviceEndpoint {
                transport: Transport::Wifi,
                serial: wifi.to_string(),
            }]
        );
        // Addressed by Wi-Fi now that USB is gone.
        assert_eq!(device.serial, wifi);
    }

    /// A device that is not reachable on any transport is not reported at all.
    #[test]
    fn a_device_with_no_transport_is_not_reported() {
        let raw = vec![raw("bc058b2c")];
        let props: HashMap<String, adb::DeviceProps> =
            [("bc058b2c".to_string(), props("bc058b2c", "CPH2773"))]
                .into_iter()
                .collect();
        let mut manager = DeviceManager {
            props,
            known: group_devices(&raw, &HashMap::new()),
        };

        manager.known = group_devices(&[], &HashMap::new());

        assert!(manager.known.is_empty());
    }
}
