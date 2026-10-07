//! USB transport.
//!
//! USB connectivity rides on the local ADB daemon (device attached over cable).
//! Discovery and attach/detach events are handled here; pairing semantics for
//! wireless differ and live in [`super::wifi`].
