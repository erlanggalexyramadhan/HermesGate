//! ADB integration boundary.
//!
//! The single module allowed to locate, spawn and parse `adb`. Other modules
//! ask this boundary for capabilities (ports to forward, devices to target)
//! instead of shelling out themselves. Not implemented at bootstrap.
