//! Wi-Fi transport.
//!
//! Wireless pairing and discovery using `adb pair`, mDNS detection of devices on the
//! local network, and scrcpy server integration. Once paired, a network device yields
//! the same connection session as a USB device, so the mirroring pipeline is unchanged.
//!
//! Workflow:
//!   1. Discover devices on the local network (mDNS/SSDP)
//!   2. Pair the device: `adb pair <ip:port> <key>`.
//!   3. Connect: `adb connect <ip:port>`.
//!   4. Forward ADB TCP: establish tunnel for the mirroring session.
//!   5. Mirroring pipeline is identical to USB transport.
//!
//! QR pairing rides on the same flow: the host displays the pairing string
//! ([`super::qr`]), the **phone's camera** scans it, the phone publishes
//! `_adb-tls-pairing._tcp` under the instance name that string requested, and
//! [`pair_over_mdns`] finishes the job with `adb pair`. The manual entry path
//! above is untouched.

use serde::Serialize;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::qr::PairingCode;

/// Service type the device publishes while its pairing server is up — the one
/// a QR pairing waits for.
pub const PAIRING_SERVICE: &str = "_adb-tls-pairing._tcp";
/// Service type the device publishes once wireless debugging is on; used after
/// pairing so the device shows up without the user typing an address.
const CONNECT_SERVICE: &str = "_adb-tls-connect._tcp";
/// Budget of one `adb mdns services` call.
const MDNS_QUERY_SECS: u64 = 15;
/// Budget of the `adb pair` call itself (TLS handshake included).
const PAIR_SECS: u64 = 60;
/// How long the phone is given to publish `_adb-tls-connect._tcp` after a
/// successful pairing before the flow reports pairing alone.
const CONNECT_GRACE: Duration = Duration::from_secs(8);

/// One instance `adb mdns services` reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsService {
    /// Instance name (`hermesgate-…`, `adb-<serial>-…`, `studio-…`).
    pub instance: String,
    /// Service type (`_adb-tls-pairing._tcp`).
    pub service_type: String,
    /// Resolved address; the command prints IPv4 only.
    pub address: SocketAddr,
}

/// Read `adb mdns services` output.
///
/// ```text
/// List of discovered mdns services
/// adb-14141FDF600081-QXjCrW  _adb-tls-pairing._tcp  192.168.86.38:33861
/// ```
///
/// The header, and every line whose last field is not an `ip:port`, are skipped.
/// The instance name is kept whole — mDNS instance names may contain spaces, so
/// only the service type (the last token before the address) is split off.
pub fn parse_mdns_services(output: &str) -> Vec<MdnsService> {
    output.lines().filter_map(parse_mdns_line).collect()
}

fn parse_mdns_line(line: &str) -> Option<MdnsService> {
    let (head, address) = line.trim().rsplit_once(char::is_whitespace)?;
    let address: SocketAddr = address.parse().ok()?;
    let (instance, service_type) = head.trim().rsplit_once(char::is_whitespace)?;
    let (instance, service_type) = (instance.trim(), service_type.trim());
    if instance.is_empty() || service_type.is_empty() {
        return None;
    }
    Some(MdnsService {
        instance: instance.to_string(),
        service_type: service_type.to_string(),
        address,
    })
}

// Transport used to reach a device.
// This is the seam that keeps USB and Wi-Fi interchangeable: the mirroring
// pipeline consumes a session, never a transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// Device reachable over USB through the local ADB daemon.
    Usb,
    /// Device reached over the network (pairing / discovery).
    Wifi,
}

impl Transport {
    /// Transport implied by an ADB serial: network devices are addressed as
    /// `ip:port`, USB devices by their hardware serial.
    pub fn for_serial(serial: &str) -> Self {
        if serial.contains(':') {
            Transport::Wifi
        } else {
            Transport::Usb
        }
    }
}

/// State of a Wi-Fi pairing session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingState {
    /// No pairing in progress.
    Idle,
    /// Device discovered; waiting for user input of pairing key.
    Discovered(SocketAddr),
    /// Pairing key entered; waiting for adb connect.
    Paired(SocketAddr),
    /// Connected over Wi-Fi.
    Connected(SocketAddr),
}

/// Metadata about a discovered device on the local network.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// IP:Port address of the device.
    pub address: SocketAddr,
    /// Device name as reported by mDNS or ADB.
    pub name: String,
    /// Serial number or unique identifier.
    pub serial: String,
}

/// Active Wi-Fi pairing manager.
#[derive(Debug)]
pub struct WifiManager {
    /// Current pairing state.
    state: std::sync::Mutex<PairingState>,
    /// Local ADB binary path for spawning pairing/connect processes.
    adb_path: std::path::PathBuf,
    /// The QR pairing code currently displayed, if any. Single use: the wait
    /// consumes it, so a stale QR can never pair.
    code: std::sync::Mutex<Option<PairingCode>>,
}

impl WifiManager {
    /// Create a new Wi-Fi manager instance.
    pub fn new(adb_path: std::path::PathBuf) -> Self {
        Self {
            state: std::sync::Mutex::new(PairingState::Idle),
            adb_path,
            code: std::sync::Mutex::new(None),
        }
    }

    /// Get the current pairing state.
    pub fn state(&self) -> std::sync::MutexGuard<'_, PairingState> {
        self.state.lock().unwrap()
    }

    /// The resolved ADB binary this manager spawns.
    pub fn adb_path(&self) -> &Path {
        &self.adb_path
    }

    /// Generate a fresh QR pairing code and remember it as the pending ticket.
    pub fn start_qr_pairing(&self) -> PairingCode {
        let code = PairingCode::generate();
        *self.code.lock().unwrap() = Some(code.clone());
        code
    }

    /// Consume the pending QR pairing code (a QR pairs at most once).
    pub fn take_qr_code(&self) -> Result<PairingCode, String> {
        self.code.lock().unwrap().take().ok_or_else(|| {
            "no QR pairing is in progress — ask for a new QR code and scan that".to_string()
        })
    }

    /// Record where a QR pairing ended: [`PairingState::Connected`] only when the
    /// phone's connect service actually answered, [`PairingState::Paired`]
    /// otherwise.
    ///
    /// The two addresses differ — the pairing port is closed as soon as the
    /// handshake finishes — so reporting `Connected` on pairing alone would
    /// claim a connection that never existed.
    pub fn note_qr_outcome(&self, paired: &str, connected: Option<&str>) {
        let Ok(paired) = paired.parse() else { return };
        let state = match connected.and_then(|address| address.parse().ok()) {
            Some(address) => PairingState::Connected(address),
            None => PairingState::Paired(paired),
        };
        *self.state.lock().unwrap() = state;
    }

    /// Discover devices on the local network via mDNS.
    ///
    /// This resolves `_adb._tcp.local.` records via mDNS to find devices
    /// that have adb forward or scrcpy server running.
    ///
    /// # Returns
    /// A vector of discovered `DeviceInfo`. Returns empty vector if no devices
    /// are found or if mDNS discovery is not available in this environment.
    ///
    /// # Note
    /// Actual mDNS discovery requires the `mdns` crate or platform-specific tools.
    /// This implementation provides the scaffold; actual discovery requires
    /// adding the `mdns` dependency and platform-specific configuration.
    pub fn discover_devices(&self) -> Vec<DeviceInfo> {
        // mDNS discovery scaffold: return empty until mdns dependency is added.
        // TODO: Add `mdns` crate and implement proper mDNS query for `_adb._tcp.local.`
        Vec::new()
    }

    /// Run one `adb` invocation with a hard timeout, returning its raw
    /// stdout and stderr.
    ///
    /// ponytail: `spawn` + `try_wait` polling instead of a second thread or
    /// an async runtime — pairing is a rare, user-initiated action, so a
    /// 200 ms poll costs nothing and keeps the command layer synchronous.
    fn run_adb(&self, args: &[&str], timeout_secs: u64) -> Result<(String, String), String> {
        run_adb(&self.adb_path, args, timeout_secs)
    }

    /// Pair with an Android device over TCP using `adb pair <address> <key>`.
    ///
    /// `key` is the pairing code the device shows under
    /// Developer options → Wireless debugging → Pair device with pairing code.
    /// adb reports success on **stdout** (`Successfully paired to …`).
    ///
    /// # Arguments
    /// - `address`: The device IP:Port to pair with (e.g. `192.168.1.100:37123`).
    /// - `key`: The pairing code shown on the device.
    /// - `timeout_secs`: Hard timeout for the adb process.
    ///
    /// # Returns
    /// `Ok(address)` if pairing succeeded; `Err` otherwise.
    pub fn pair_device(
        &self,
        address: SocketAddr,
        key: &str,
        timeout_secs: u64,
    ) -> Result<SocketAddr, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("pairing key is empty".to_string());
        }

        let (stdout, stderr) = self.run_adb(&["pair", &address.to_string(), key], timeout_secs)?;

        // adb prints "Successfully paired to 192.168.x.x:xxxxx [guid=…]" on success.
        if stdout.contains("Successfully paired") {
            let mut state = self.state.lock().unwrap();
            *state = PairingState::Paired(address);
            Ok(address)
        } else {
            Err(format!(
                "adb pair failed: {} {}",
                stdout.trim(),
                stderr.trim()
            ))
        }
    }

    /// Connect to a previously paired Android device over TCP using `adb connect`.
    ///
    /// adb prints `connected to …` or `already connected to …` on stdout.
    ///
    /// # Arguments
    /// - `address`: The device IP:Port to connect with (e.g. `192.168.1.100:5555`).
    ///
    /// # Returns
    /// `Ok(address)` if connection succeeded; `Err` if adb command failed.
    pub fn connect_device(&self, address: SocketAddr) -> Result<SocketAddr, String> {
        let (stdout, stderr) = self.run_adb(&["connect", &address.to_string()], 30)?;

        if stdout.contains("connected to") {
            let mut state = self.state.lock().unwrap();
            *state = PairingState::Connected(address);
            Ok(address)
        } else {
            Err(format!(
                "adb connect failed: {} {}",
                stdout.trim(),
                stderr.trim()
            ))
        }
    }

    /// Disconnect a Wi-Fi ADB connection (`adb disconnect`).
    ///
    /// # Arguments
    /// - `address`: The device IP:Port to disconnect.
    ///
    /// # Returns
    /// `Ok(address)` if disconnection succeeded; `Err` if adb command failed.
    pub fn disconnect_device(&self, address: SocketAddr) -> Result<SocketAddr, String> {
        let (stdout, stderr) = self.run_adb(&["disconnect", &address.to_string()], 30)?;

        if stdout.contains("disconnected") {
            let mut state = self.state.lock().unwrap();
            *state = PairingState::Idle;
            Ok(address)
        } else {
            Err(format!(
                "adb disconnect failed: {} {}",
                stdout.trim(),
                stderr.trim()
            ))
        }
    }

    /// Cycle: pair then connect a device.
    ///
    /// # Arguments
    /// - `address`: The device IP:Port to pair and connect with.
    /// - `key`: The pairing code shown on the device.
    /// - `timeout_secs`: Hard timeout for the pairing step.
    ///
    /// # Returns
    /// `Ok(address)` if both pairing and connection succeeded; `Err` otherwise.
    pub fn pair_and_connect(
        &self,
        address: SocketAddr,
        key: &str,
        timeout_secs: u64,
    ) -> Result<SocketAddr, String> {
        self.pair_device(address, key, timeout_secs)?;
        self.connect_device(address)
    }
}

/// Outcome of a QR pairing attempt, as the UI reports it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrPairingOutcome {
    /// `ip:port` the phone answered the pairing on (its pairing port).
    pub paired: String,
    /// `ip:port` that was `adb connect`ed afterwards, when the phone published
    /// its connect service in time.
    pub connected: Option<String>,
}

/// Run one `adb` invocation with a hard timeout, returning its raw stdout and
/// stderr.
///
/// ponytail: `spawn` + `try_wait` polling instead of a second thread or an
/// async runtime — pairing is a rare, user-initiated action, so a 200 ms poll
/// costs nothing and keeps the command layer synchronous. It is a free function
/// so the QR wait can also run it on a blocking thread with nothing borrowed.
fn run_adb(adb: &Path, args: &[&str], timeout_secs: u64) -> Result<(String, String), String> {
    let mut child = crate::core::adb::command(adb)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to execute adb {}: {}", args[0], e))?;

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("adb {} timed out after {}s", args[0], timeout_secs));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(format!("failed while waiting for adb {}: {}", args[0], e)),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to read adb output: {}", e))?;
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// `adb mdns services`, parsed. An unavailable mDNS backend is an error; an
/// empty list is not — it only means the phone has not answered yet.
fn mdns_services(adb: &Path) -> Result<Vec<MdnsService>, String> {
    let (stdout, stderr) = run_adb(adb, &["mdns", "services"], MDNS_QUERY_SECS)?;
    if !stdout.contains("List of discovered mdns services") {
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(format!("`adb mdns services` is unavailable: {detail}"));
    }
    Ok(parse_mdns_services(&stdout))
}

/// Wait for the phone that scanned `code` to publish its pairing server, pair
/// with it, then (best effort) connect to it.
///
/// Free function on purpose: the command runs it on a blocking thread, so it
/// borrows nothing and owns everything it needs.
pub fn pair_over_mdns(
    adb: &Path,
    code: &PairingCode,
    timeout: Duration,
) -> Result<QrPairingOutcome, String> {
    let deadline = Instant::now() + timeout;
    // Fail fast when mDNS itself is unavailable; a single failed poll after
    // that is transient and not worth aborting the attempt over.
    let mut polled = mdns_services(adb)?;

    let pairing = loop {
        if let Some(service) = polled
            .iter()
            .find(|service| {
                service.service_type == PAIRING_SERVICE && service.instance == code.service()
            })
            .cloned()
        {
            break service;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "no phone claimed that QR code within {}s — keep \"Pair device with QR code\" \
                 open on the phone and make sure both are on the same network: mDNS carries the \
                 address, and guest or corporate Wi-Fi blocks it",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_secs(1));
        polled = mdns_services(adb).unwrap_or_default();
    };

    let address = pairing.address.to_string();
    let (stdout, stderr) = run_adb(adb, &["pair", &address, code.password()], PAIR_SECS)?;
    if !stdout.contains("Successfully paired") {
        return Err(format!(
            "adb pair failed: {} {}",
            stdout.trim(),
            stderr.trim()
        ));
    }

    // Best effort: wireless debugging also publishes `_adb-tls-connect._tcp`, and
    // connecting it is what puts the device in `adb devices` without the user
    // typing an address. adb usually auto-connects on its own, so this is the
    // fallback — bounded, so it can never hold the flow.
    let connected = connect_service(adb, pairing.address, Instant::now() + CONNECT_GRACE);
    Ok(QrPairingOutcome {
        paired: address,
        connected,
    })
}

/// `adb connect` the connect service of the phone that was just paired, for at
/// most the given grace period. Returns the address that answered.
fn connect_service(adb: &Path, phone: SocketAddr, deadline: Instant) -> Option<String> {
    loop {
        if let Ok(services) = mdns_services(adb) {
            let found = services
                .into_iter()
                .find(|service| {
                    service.service_type == CONNECT_SERVICE && service.address.ip() == phone.ip()
                })
                .map(|service| service.address.to_string());
            if let Some(address) = found {
                if let Ok((stdout, _)) = run_adb(adb, &["connect", &address], 30) {
                    if stdout.contains("connected to") {
                        return Some(address);
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `adb mdns services` before the phone answers: header only, or nothing at
    /// all.
    #[test]
    fn an_empty_mdns_list_parses_to_nothing() {
        assert!(parse_mdns_services("List of discovered mdns services\n").is_empty());
        assert!(parse_mdns_services("").is_empty());
    }

    /// Every service type a wireless-debugging phone publishes, plus a line that
    /// is not a service (`adb mdns services` prints errors on stdout too).
    #[test]
    fn reads_the_services_adb_prints() {
        let services = parse_mdns_services(
            "List of discovered mdns services\n\
             adb-14141FDF600081-QXjCrW  _adb-tls-pairing._tcp  192.168.86.38:33861\n\
             adb-14141FDF600081-QXjCrW  _adb-tls-url-pairing._tcp  192.168.86.38:33859\n\
             adb-14141FDF600081-QXjCrW  _adb-tls-connect._tcp  192.168.86.38:5555\n\
             ERROR: could not resolve\n",
        );
        assert_eq!(services.len(), 3);
        assert_eq!(services[0].instance, "adb-14141FDF600081-QXjCrW");
        assert_eq!(services[0].service_type, PAIRING_SERVICE);
        assert_eq!(services[0].address.to_string(), "192.168.86.38:33861");
        assert_eq!(services[1].service_type, "_adb-tls-url-pairing._tcp");
        assert_eq!(services[2].service_type, CONNECT_SERVICE);
    }

    /// An mDNS instance name may contain spaces; only the service type is split
    /// off the line, so the name this host asked for survives intact.
    #[test]
    fn keeps_an_instance_name_with_spaces_whole() {
        let services = parse_mdns_services(
            "List of discovered mdns services\n\
             hermesgate-a b  _adb-tls-pairing._tcp  10.0.0.5:12345\n",
        );
        assert_eq!(services[0].instance, "hermesgate-a b");
        assert_eq!(services[0].address.to_string(), "10.0.0.5:12345");
    }

    /// A QR pairing that only completed the handshake reports `Paired`: the
    /// pairing port is closed once the handshake is done, so claiming a
    /// connection to it would be false.
    #[test]
    fn a_qr_pairing_reports_paired_until_it_is_connected() {
        let manager = WifiManager::new(std::path::PathBuf::from("adb"));
        manager.note_qr_outcome("192.168.1.42:37123", None);
        assert_eq!(
            *manager.state(),
            PairingState::Paired("192.168.1.42:37123".parse().unwrap())
        );
        manager.note_qr_outcome("192.168.1.42:37123", Some("192.168.1.42:5555"));
        assert_eq!(
            *manager.state(),
            PairingState::Connected("192.168.1.42:5555".parse().unwrap())
        );
    }

    /// A ticket is single use: the wait consumes it, so a QR left on screen from
    /// an earlier attempt can never pair a device.
    #[test]
    fn a_qr_ticket_is_spent_once() {
        let manager = WifiManager::new(std::path::PathBuf::from("adb"));
        let code = manager.start_qr_pairing();
        assert_eq!(manager.take_qr_code().unwrap(), code);
        assert!(manager.take_qr_code().is_err());
    }

    /// Asking for a new QR code replaces the pending one.
    #[test]
    fn a_new_qr_ticket_replaces_the_previous_one() {
        let manager = WifiManager::new(std::path::PathBuf::from("adb"));
        manager.start_qr_pairing();
        let second = manager.start_qr_pairing();
        assert_eq!(manager.take_qr_code().unwrap(), second);
    }

    #[test]
    fn test_transport_enum() {
        assert_eq!(Transport::for_serial("serial123"), Transport::Usb);
        assert_eq!(
            Transport::for_serial("192.168.1.100:22100"),
            Transport::Wifi
        );
    }

    #[test]
    fn test_wifi_manager_creation() {
        let manager = WifiManager::new(std::path::PathBuf::from("adb"));
        let guard = manager.state();
        assert_eq!(*guard, PairingState::Idle);
    }

    #[test]
    fn test_device_info_creation() {
        let info = DeviceInfo {
            address: "192.168.1.100:22100".parse().unwrap(),
            name: "Android Device".to_string(),
            serial: "EMULATOR".to_string(),
        };
        // Just verify it can be created; address parsing is validated above
        assert!(!info.name.is_empty());
    }
}
