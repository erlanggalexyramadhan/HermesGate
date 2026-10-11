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

use serde::Serialize;
use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
}

impl WifiManager {
    /// Create a new Wi-Fi manager instance.
    pub fn new(adb_path: std::path::PathBuf) -> Self {
        Self {
            state: std::sync::Mutex::new(PairingState::Idle),
            adb_path,
        }
    }

    /// Get the current pairing state.
    pub fn state(&self) -> std::sync::MutexGuard<'_, PairingState> {
        self.state.lock().unwrap()
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
        let mut child = Command::new(&self.adb_path)
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

#[cfg(test)]
mod tests {
    use super::*;

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
