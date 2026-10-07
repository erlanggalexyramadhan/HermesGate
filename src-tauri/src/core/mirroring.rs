//! Mirroring engine boundary.
//!
//! Owns mirroring sessions: start/stop streaming for the active connection
//! session ([`crate::core::connection`]) and feed frames into the shared
//! render pipeline in [`crate::core::video`]. USB and Wi-Fi reach this module
//! through the same session type, so the pipeline is transport-agnostic.
