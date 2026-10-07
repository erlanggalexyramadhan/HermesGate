//! Input engine boundary.
//!
//! Turns desktop input (pointer, wheel, keyboard, gestures) into device events
//! and sends them over the active connection session. Input never travels
//! through the webview as frame-by-frame traffic; the UI only signals intent
//! through Tauri commands/events.
