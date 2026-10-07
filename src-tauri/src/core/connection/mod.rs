//! Connection Manager — the transport layer.
//!
//! The only layer that knows *how* HermesGate reaches a device. USB and Wi-Fi
//! differ here and nowhere else: everything downstream ([`crate::core::mirroring`],
//! [`crate::core::input`], [`crate::core::clipboard`]) operates on the same
//! connection session, so both transports share one mirroring/input pipeline.

use serde::Serialize;

pub mod usb;
pub mod wifi;

/// Transport used to reach a device.
///
/// This is the seam that keeps USB and Wi-Fi interchangeable: the mirroring
/// pipeline consumes a session, never a transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// Device reachable over USB through the local ADB daemon.
    Usb,
    /// Device reached over the network (pairing / discovery).
    Wifi,
}
