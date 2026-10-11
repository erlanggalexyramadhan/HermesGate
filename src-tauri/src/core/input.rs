//! Input engine boundary.
//!
//! Turns desktop input (pointer, wheel, keyboard, gestures) into device events
//! and sends them over the active connection session. Input never travels
//! through the webview as frame-by-frame traffic; the UI only signals intent
//! through Tauri commands/events.
//!
//! Implemented scope: pointer **and navigation keys** — left-button
//! press/drag/release, wheel scroll, and Back/Home/Recents key presses, over
//! scrcpy's control channel (`control=true`), encoded with the control protocol
//! the server decodes (`app/src/control_msg.c`, scrcpy v5.0 — the version this
//! app ships): `INJECT_TOUCH_EVENT` (type 2), `INJECT_SCROLL_EVENT` (type 3) and
//! `INJECT_KEYCODE` (type 0). Free-text injection and full keyboard input are
//! not implemented.

use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// `ControlMessage.TYPE_INJECT_TOUCH_EVENT`.
const TYPE_INJECT_TOUCH_EVENT: u8 = 2;
/// `ControlMessage.TYPE_INJECT_SCROLL_EVENT`.
const TYPE_INJECT_SCROLL_EVENT: u8 = 3;
/// `ControlMessage.TYPE_INJECT_KEYCODE`.
const TYPE_INJECT_KEYCODE: u8 = 0;
/// `KeyEvent.ACTION_DOWN` (`android_keyevent_action`).
pub const KEY_ACTION_DOWN: u8 = 0;
/// `KeyEvent.ACTION_UP`.
pub const KEY_ACTION_UP: u8 = 1;
/// `AKEYCODE_HOME` (`android/keycodes.h`).
pub const KEYCODE_HOME: u32 = 3;
/// `AKEYCODE_BACK`.
pub const KEYCODE_BACK: u32 = 4;
/// `AKEYCODE_APP_SWITCH` — the Recents key.
pub const KEYCODE_APP_SWITCH: u32 = 187;

/// `MotionEvent.ACTION_DOWN`.
pub const ACTION_DOWN: u8 = 0;
/// `MotionEvent.ACTION_UP`.
pub const ACTION_UP: u8 = 1;
/// `MotionEvent.ACTION_MOVE`.
pub const ACTION_MOVE: u8 = 2;

/// The pointer id scrcpy's own client uses for the mouse.
const MOUSE_POINTER_ID: u64 = 1;
/// `MotionEvent.BUTTON_PRIMARY` / `BUTTON_NONE`.
pub const BUTTON_PRIMARY: u32 = 1;
pub const BUTTON_NONE: u32 = 0;
/// `MK_LBUTTON` in the `WM_MOUSEMOVE` key state.
const MK_LBUTTON: usize = 0x0001;

/// Full pressure for a finger down/move; a released finger reports none.
const PRESSURE_MAX: u16 = u16::MAX;

/// One wheel notch in `WM_MOUSEWHEEL` (`WHEEL_DELTA`).
const WHEEL_DELTA: f32 = 120.0;

/// A touch event: `type(1) action(1) pointerId(8) x(4) y(4) width(2)
/// height(2) pressure(2) actionButton(4) buttons(4)` = 32 bytes, all
/// big-endian (`write_position` + `sc_control_msg_serialize`).
///
/// `x`/`y` are device pixels and `picture` is the frame size they are
/// relative to — the mirrored picture, which is the device screen unless the
/// session downscales it.
pub fn touch_event(action: u8, x: i32, y: i32, picture: (u32, u32), buttons: u32) -> [u8; 32] {
    let mut buffer = [0u8; 32];
    buffer[0] = TYPE_INJECT_TOUCH_EVENT;
    buffer[1] = action;
    buffer[2..10].copy_from_slice(&MOUSE_POINTER_ID.to_be_bytes());
    buffer[10..14].copy_from_slice(&x.to_be_bytes());
    buffer[14..18].copy_from_slice(&y.to_be_bytes());
    buffer[18..20].copy_from_slice(&picture_u16(picture.0).to_be_bytes());
    buffer[20..22].copy_from_slice(&picture_u16(picture.1).to_be_bytes());
    let pressure = if action == ACTION_UP { 0 } else { PRESSURE_MAX };
    buffer[22..24].copy_from_slice(&pressure.to_be_bytes());
    // The action button is the button that caused this event; the button
    // state is what is held while it happens.
    buffer[24..28].copy_from_slice(&buttons.to_be_bytes());
    buffer[28..32].copy_from_slice(&buttons.to_be_bytes());
    buffer
}

/// A scroll event: `type(1) x(4) y(4) width(2) height(2) hScroll(2)
/// vScroll(2) buttons(4)` = 21 bytes. Both scroll axes are normalized to
/// `[-1, 1]` (one notch is 1.0) as fixed point, exactly like scrcpy's client.
pub fn scroll_event(
    x: i32,
    y: i32,
    picture: (u32, u32),
    hscroll: f32,
    vscroll: f32,
    buttons: u32,
) -> [u8; 21] {
    let mut buffer = [0u8; 21];
    buffer[0] = TYPE_INJECT_SCROLL_EVENT;
    buffer[1..5].copy_from_slice(&x.to_be_bytes());
    buffer[5..9].copy_from_slice(&y.to_be_bytes());
    buffer[9..11].copy_from_slice(&picture_u16(picture.0).to_be_bytes());
    buffer[11..13].copy_from_slice(&picture_u16(picture.1).to_be_bytes());
    buffer[13..15].copy_from_slice(&fixed(hscroll).to_be_bytes());
    buffer[15..17].copy_from_slice(&fixed(vscroll).to_be_bytes());
    buffer[17..21].copy_from_slice(&buttons.to_be_bytes());
    buffer
}

/// `sc_float_to_i16fp`: normalized `[-1, 1]` as signed 16-bit fixed point.
fn fixed(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

/// The display size field is 16 bits wide on the wire.
fn picture_u16(value: u32) -> u16 {
    value.min(u16::MAX as u32) as u16
}

/// Desktop client coordinates → device picture coordinates.
///
/// The window shows the whole picture stretched across its client area, so
/// the mapping is linear and clamped to the picture.
pub fn map(client: (i32, i32), client_size: (u32, u32), picture: (u32, u32)) -> (i32, i32) {
    if client_size.0 == 0 || client_size.1 == 0 || picture.0 == 0 || picture.1 == 0 {
        return (0, 0);
    }
    let scale = |point: i32, client: u32, device: u32| {
        let mapped = (point as f64 * device as f64 / client as f64).round() as i64;
        mapped.clamp(0, device as i64 - 1) as i32
    };
    (
        scale(client.0, client_size.0, picture.0),
        scale(client.1, client_size.1, picture.1),
    )
}

/// A key event: `type(1) action(1) keycode(4) repeat(4) metastate(4)`.
///
/// 14 bytes big-endian, exactly what scrcpy v5.0's serializer writes for
/// `INJECT_KEYCODE` — note that the action is one byte here, unlike the touch
/// event's four. `repeat` and `metastate` are 0 for a plain press, which is what
/// the scrcpy client itself sends.
pub fn key_event(action: u8, keycode: u32) -> [u8; 14] {
    let mut buffer = [0u8; 14];
    buffer[0] = TYPE_INJECT_KEYCODE;
    buffer[1] = action;
    buffer[2..6].copy_from_slice(&keycode.to_be_bytes());
    buffer[6..10].copy_from_slice(&0u32.to_be_bytes());
    buffer[10..14].copy_from_slice(&0u32.to_be_bytes());
    buffer
}

/// One wheel notch of scroll, as the normalized value the protocol wants.
pub fn wheel(value: i32) -> f32 {
    value as f32 / WHEEL_DELTA
}

/// The control socket of a live session.
///
/// With `tunnel_forward=true` the scrcpy server accepts the video socket
/// first and the control socket second, so this is the *second* connection to
/// the session's forward. The server blocks in `accept()` until it is there:
/// a session without it never streams.
pub struct Controller {
    writer: Mutex<TcpStream>,
    /// The socket failed once: keep sending (it may recover) but say it once
    /// instead of logging every mouse move.
    reported: AtomicBool,
}

impl Controller {
    /// Connect the control socket of the session forwarded on `port`.
    pub fn connect(port: u16) -> Result<Self, String> {
        let stream = TcpStream::connect(("127.0.0.1", port))
            .map_err(|error| format!("control socket refused on 127.0.0.1:{port}: {error}"))?;
        let _ = stream.set_nodelay(true);
        Ok(Self {
            writer: Mutex::new(stream),
            reported: AtomicBool::new(false),
        })
    }

    /// Press/release/drag the pointer at device coordinates.
    pub fn touch(&self, action: u8, x: i32, y: i32, picture: (u32, u32), buttons: u32) {
        self.send(&touch_event(action, x, y, picture, buttons));
    }

    /// Scroll at device coordinates; one notch is 1.0 per axis.
    pub fn scroll(&self, x: i32, y: i32, picture: (u32, u32), hscroll: f32, vscroll: f32) {
        self.send(&scroll_event(x, y, picture, hscroll, vscroll, BUTTON_NONE));
    }

    /// Press and release one key, as a pair of events — the server decodes both
    /// and the device's input pipeline needs the `UP` to complete the gesture.
    pub fn key(&self, keycode: u32) {
        self.send(&key_event(KEY_ACTION_DOWN, keycode));
        self.send(&key_event(KEY_ACTION_UP, keycode));
    }

    /// Push one control message. A failed write ends nothing by itself: the
    /// video stream is what reports the session, so input just stops working.
    fn send(&self, message: &[u8]) {
        let Ok(mut stream) = self.writer.lock() else {
            return;
        };
        if let Err(error) = stream.write_all(message) {
            if !self.reported.swap(true, Ordering::SeqCst) {
                eprintln!("mirror: input is not reaching the device: {error}");
            }
        }
    }
}

/// The control socket of the running session, if input is available.
static ACTIVE: Mutex<Option<Arc<Controller>>> = Mutex::new(None);

/// The session with this connection owns the channel.
pub fn attach(controller: Arc<Controller>) {
    if let Ok(mut active) = ACTIVE.lock() {
        *active = Some(controller);
    }
}

/// The channel is gone; a later session attaches its own.
pub fn detach() {
    if let Ok(mut active) = ACTIVE.lock() {
        *active = None;
    }
}

/// The controller to send input through, or `None` while no session runs.
pub fn active() -> Option<Arc<Controller>> {
    ACTIVE.lock().ok().and_then(|active| active.clone())
}

/// `WM_MOUSEMOVE` key state: is the left button held?
pub fn dragging(key_state: usize) -> bool {
    key_state & MK_LBUTTON != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_down_matches_the_control_protocol() {
        let message = touch_event(ACTION_DOWN, 360, 802, (720, 1604), BUTTON_PRIMARY);
        assert_eq!(message.len(), 32);
        assert_eq!(message[0], 2);
        assert_eq!(message[1], ACTION_DOWN);
        assert_eq!(&message[2..10], &1u64.to_be_bytes());
        assert_eq!(&message[10..14], &360i32.to_be_bytes());
        assert_eq!(&message[14..18], &802i32.to_be_bytes());
        assert_eq!(&message[18..20], &720u16.to_be_bytes());
        assert_eq!(&message[20..22], &1604u16.to_be_bytes());
        assert_eq!(&message[22..24], &u16::MAX.to_be_bytes());
        assert_eq!(&message[24..28], &1u32.to_be_bytes());
        assert_eq!(&message[28..32], &1u32.to_be_bytes());
    }

    #[test]
    fn a_lifted_finger_reports_no_pressure_and_no_button() {
        let message = touch_event(ACTION_UP, 10, 20, (720, 1604), BUTTON_NONE);
        assert_eq!(message[1], ACTION_UP);
        assert_eq!(&message[22..24], &0u16.to_be_bytes());
        assert_eq!(&message[24..28], &0u32.to_be_bytes());
        assert_eq!(&message[28..32], &0u32.to_be_bytes());
    }

    #[test]
    fn a_key_press_matches_the_control_protocol() {
        let message = key_event(KEY_ACTION_DOWN, KEYCODE_BACK);
        assert_eq!(message.len(), 14);
        assert_eq!(message[0], 0);
        assert_eq!(message[1], KEY_ACTION_DOWN);
        assert_eq!(&message[2..6], &4u32.to_be_bytes(), "the Back key");
        assert_eq!(&message[6..10], &0u32.to_be_bytes(), "repeat");
        assert_eq!(&message[10..14], &0u32.to_be_bytes(), "metastate");
        assert_eq!(key_event(KEY_ACTION_UP, KEYCODE_HOME)[1], KEY_ACTION_UP);
        // The keys the session panel offers.
        assert_eq!(
            (KEYCODE_HOME, KEYCODE_BACK, KEYCODE_APP_SWITCH),
            (3, 4, 187)
        );
    }

    #[test]
    fn a_drag_keeps_the_button_state() {
        let message = touch_event(ACTION_MOVE, 5, 6, (720, 1604), BUTTON_PRIMARY);
        assert_eq!(message[1], ACTION_MOVE);
        assert_eq!(&message[28..32], &1u32.to_be_bytes());
    }

    #[test]
    fn scroll_normalizes_one_notch_to_full_scale() {
        let message = scroll_event(100, 200, (720, 1604), 0.0, 1.0, BUTTON_NONE);
        assert_eq!(message.len(), 21);
        assert_eq!(message[0], 3);
        assert_eq!(&message[1..5], &100i32.to_be_bytes());
        assert_eq!(&message[5..9], &200i32.to_be_bytes());
        assert_eq!(&message[9..11], &720u16.to_be_bytes());
        assert_eq!(&message[11..13], &1604u16.to_be_bytes());
        assert_eq!(&message[13..15], &0i16.to_be_bytes());
        assert_eq!(&message[15..17], &i16::MAX.to_be_bytes());
        assert_eq!(&message[17..21], &0u32.to_be_bytes());
    }

    #[test]
    fn scroll_clamps_out_of_range_axes() {
        assert_eq!(fixed(4.0), i16::MAX);
        assert_eq!(fixed(-4.0), -i16::MAX);
        assert_eq!(fixed(0.5), (0.5 * i16::MAX as f32) as i16);
        assert_eq!(wheel(120), 1.0);
        assert_eq!(wheel(-120), -1.0);
    }

    #[test]
    fn the_picture_field_saturates_instead_of_wrapping() {
        assert_eq!(picture_u16(u32::MAX), u16::MAX);
    }

    #[test]
    fn a_window_maps_linearly_onto_the_picture() {
        // A window the same size as the picture is a 1:1 mapping.
        assert_eq!(map((360, 802), (720, 1604), (720, 1604)), (360, 802));
        // Half the size: the bottom-right corner is the last picture pixel.
        assert_eq!(map((359, 801), (360, 802), (720, 1604)), (718, 1602));
        assert_eq!(map((0, 0), (360, 802), (720, 1604)), (0, 0));
    }

    #[test]
    fn a_pointer_outside_the_client_area_stays_inside_the_picture() {
        // Capture lets the drag leave the window; the device must not see it.
        assert_eq!(map((-40, 900), (360, 802), (720, 1604)), (0, 1603));
        assert_eq!(map((400, -10), (360, 802), (720, 1604)), (719, 0));
    }

    #[test]
    fn a_degenerate_geometry_never_panics() {
        assert_eq!(map((10, 10), (0, 0), (720, 1604)), (0, 0));
        assert_eq!(map((10, 10), (360, 802), (0, 0)), (0, 0));
    }

    #[test]
    fn only_the_left_button_counts_as_a_drag() {
        assert!(dragging(0x0001));
        assert!(!dragging(0x0002)); // MK_RBUTTON alone
        assert!(!dragging(0));
    }
}
