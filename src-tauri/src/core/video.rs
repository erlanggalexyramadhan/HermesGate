//! Video: decoder and renderer boundary.
//!
//! Receives encoded frames from [`crate::core::mirroring`], decodes natively
//! and renders on the GPU. One decoded output may be presented by several
//! windows (the main preview and a future clean mirror window) without
//! decoding the stream twice. Frame data stays in native code and never
//! crosses the JavaScript boundary.
