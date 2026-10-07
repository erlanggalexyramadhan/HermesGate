//! Device Manager boundary.
//!
//! Owns device identity and lifecycle: what devices HermesGate knows about,
//! which one is active, and whether it is reachable. Connection details come
//! from [`crate::core::connection`]; this module only describes the device.
