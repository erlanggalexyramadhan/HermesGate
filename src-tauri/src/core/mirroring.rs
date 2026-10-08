//! Mirroring engine: session lifecycle for screen streaming.
//!
//! Pipeline: the bundled `adb` pushes the official scrcpy server (v5.0,
//! Apache-2.0, shipped in `binaries/`) to the device, starts it with
//! `app_process` and forwards its video socket to a loopback TCP port. The
//! connected socket is handed to [`crate::core::video`], which decodes and
//! presents natively. No protocol is implemented here — scrcpy's server owns
//! capture/encoding; this module owns adb orchestration and session state.
//!
//! Server launch flags (verified against scrcpy v5.0 `Options.java`):
//! `raw_stream=true` disables device meta, frame meta and the dummy byte, so
//! the socket carries pure H.264 Annex-B; `control=false` keeps a single
//! (video) connection; `tunnel_forward=true` makes the server listen on
//! `localabstract:scrcpy`, which `adb forward` reaches.
//!
//! USB transport only for now. `adb forward` is transport-agnostic, so a
//! future Wi-Fi transport replaces the setup here, not the pipeline.

use crate::core::adb;
use crate::core::video::{self, PlaybackControl, StreamEnd};
use serde::Serialize;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Local abstract socket name the scrcpy server listens on (protocol-fixed
/// when `scid` is left at its default).
const SOCKET_NAME: &str = "scrcpy";
/// Loopback port for the forwarded video socket. One mirror session at a
/// time, so a fixed port is enough and keeps the forward inspectable.
const FORWARD_PORT: u16 = 27183;
/// scrcpy server jar shipped in `binaries/` (Apache-2.0, `NOTICE.txt`).
const SERVER_FILE: &str = "scrcpy-server-v5.0";
/// scrcpy server entry class (fixed by the server artifact).
const SERVER_CLASS: &str = "com.genymobile.scrcpy.Server";
/// Server protocol version argument; must match the shipped jar.
const SERVER_VERSION: &str = "5.0";
/// Remote path the server jar is pushed to.
const REMOTE_JAR: &str = "/data/local/tmp/scrcpy-server.jar";
/// Handshake budget: connect, then accept the stream only once the forward
/// actually holds (the server may take a moment to listen).
const CONNECT_ATTEMPTS: u32 = 15;
const CONNECT_INTERVAL: Duration = Duration::from_millis(300);

/// Basic quality knobs forwarded verbatim to scrcpy (`0` = server default:
/// native resolution, 8 Mbit/s, uncapped fps).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorQuality {
    /// Longest screen edge in pixels (`max_size`), `0` = keep native.
    pub max_size: u32,
    /// Video bitrate in bit/s (`video_bit_rate`), `0` = server default.
    pub bitrate: u32,
    /// Frame rate cap (`max_fps`), `0` = uncapped.
    pub max_fps: u32,
}

/// Session state for the UI, pushed on change via `mirror-changed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorStatus {
    /// A session exists and has not finished yet.
    pub running: bool,
    /// `idle` | `starting` | `running` | `stopped` | `failed`.
    pub phase: &'static str,
    /// Human-readable failure detail (empty while healthy).
    pub reason: String,
    /// Last decoded frame size (persists after a session ends).
    pub width: u32,
    pub height: u32,
    /// Serial of the mirrored device.
    pub serial: String,
}

impl Default for MirrorStatus {
    fn default() -> Self {
        Self {
            running: false,
            phase: "idle",
            reason: String::new(),
            width: 0,
            height: 0,
            serial: String::new(),
        }
    }
}

/// One live mirroring session: server process, decode thread, control flags.
struct Session {
    serial: String,
    control: Arc<PlaybackControl>,
    status: Arc<Mutex<MirrorStatus>>,
    child: Option<Child>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Session {
    /// Bring up the full pipeline. Every failure path after the first device
    /// command rolls the device back (server killed, forward removed).
    fn start(serial: &str, quality: MirrorQuality) -> Result<Self, String> {
        let serial = serial.trim();
        if serial.is_empty() {
            return Err("no device selected".to_string());
        }
        let adb = adb::resolve()?;
        let jar = server_jar()?;

        // The device must be authorized and online before anything else.
        let devices = adb::list_devices(&adb.path)?;
        match devices.iter().find(|device| device.serial == serial) {
            None => return Err(format!("device {serial} is not connected")),
            Some(device) if device.state != "device" => {
                return Err(format!(
                    "device {serial} is {} — unlock it and allow USB debugging",
                    device.state
                ));
            }
            Some(_) => {}
        }

        let forward_arg = format!("tcp:{FORWARD_PORT}");
        let socket_arg = format!("localabstract:{SOCKET_NAME}");
        let cleanup = || {
            let _ = adb::run(
                &adb.path,
                &["-s", serial, "shell", "pkill", "-f", SERVER_CLASS],
            );
            let _ = adb::run(&adb.path, &["--remove-forward", &forward_arg]);
        };

        let launched = (|| -> Result<Child, String> {
            // Official scrcpy server, pushed once per start.
            let jar_path = jar.to_string_lossy().into_owned();
            adb::run(&adb.path, &["-s", serial, "push", &jar_path, REMOTE_JAR])?;
            // Replace any leftover server from a previous session.
            let _ = adb::run(
                &adb.path,
                &["-s", serial, "shell", "pkill", "-f", SERVER_CLASS],
            );
            // One loopback forward for the video socket. Transport seam: a
            // future Wi-Fi transport replaces this setup, not the pipeline.
            let _ = adb::run(&adb.path, &["--remove-forward", &forward_arg]);
            adb::run(
                &adb.path,
                &["-s", serial, "forward", &forward_arg, &socket_arg],
            )?;
            // Session-lifetime server process; killed on stop or teardown.
            let args = server_args(serial, quality);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            adb::spawn(&adb.path, &refs)
        })();

        let mut child = match launched {
            Ok(child) => child,
            Err(error) => {
                cleanup();
                return Err(error);
            }
        };

        let stream = match wait_for_forward() {
            Ok(stream) => stream,
            Err(error) => {
                cleanup();
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };

        let control = Arc::new(PlaybackControl::default());
        let status = Arc::new(Mutex::new(MirrorStatus {
            running: true,
            phase: "starting",
            ..MirrorStatus::default()
        }));
        let thread_status = status.clone();
        let thread_control = control.clone();
        let adb_path = adb.path.clone();
        let title = format!("HermesGate Mirror — {serial}");
        let spawned = std::thread::Builder::new()
            .name("mirror-session".to_string())
            .spawn(move || {
                // Decodes and presents until the session ends; `on_size`
                // publishes the first frame — that is when it is "running".
                let end = video::play(stream.into(), thread_control, &title, |width, height| {
                    if let Ok(mut guard) = thread_status.lock() {
                        guard.width = width;
                        guard.height = height;
                        guard.phase = "running";
                        guard.reason.clear();
                    }
                });
                // Release the transport whatever ended the stream.
                let _ = adb::run(
                    &adb_path,
                    &["--remove-forward", &format!("tcp:{FORWARD_PORT}")],
                );
                let (phase, reason) = match end {
                    StreamEnd::Stopped => ("stopped", String::new()),
                    StreamEnd::UserClosed => ("stopped", "mirror window closed".to_string()),
                    StreamEnd::Disconnected => (
                        "failed",
                        "device disconnected or the stream ended".to_string(),
                    ),
                    StreamEnd::Failed(message) => ("failed", message),
                };
                if let Ok(mut guard) = thread_status.lock() {
                    guard.running = false;
                    guard.phase = phase;
                    guard.reason = reason;
                }
            })
            .map_err(|error| format!("cannot start mirror session: {error}"));
        let thread = match spawned {
            Ok(thread) => thread,
            Err(error) => {
                cleanup();
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };

        Ok(Self {
            serial: serial.to_string(),
            control,
            status,
            child: Some(child),
            thread: Some(thread),
        })
    }

    /// Stop the decode thread, then reap the server process and forward.
    fn shutdown(&mut self) {
        self.control.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Ok(adb) = adb::resolve() {
            let _ = adb::run(
                &adb.path,
                &["-s", &self.serial, "shell", "pkill", "-f", SERVER_CLASS],
            );
            let _ = adb::run(
                &adb.path,
                &["--remove-forward", &format!("tcp:{FORWARD_PORT}")],
            );
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn snapshot(&self) -> MirrorStatus {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn is_running(&self) -> bool {
        self.snapshot().running
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// At most one session at a time (Phase 2 scope: single mirrored device).
static ACTIVE: Mutex<Option<Session>> = Mutex::new(None);

fn active() -> std::sync::MutexGuard<'static, Option<Session>> {
    ACTIVE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start mirroring `serial`. Fails when a session is already running or any
/// pipeline step (device check, push, forward, server, transport) fails.
pub fn request_start(serial: &str, quality: MirrorQuality) -> Result<(), String> {
    let mut active = active();
    if let Some(existing) = active.as_ref() {
        if existing.is_running() {
            return Err("a mirror session is already running".to_string());
        }
        // Retire the finished session before replacing it (its cleanup must
        // not race the new server on the same device).
        if let Some(mut stale) = active.take() {
            stale.shutdown();
        }
    }
    *active = Some(Session::start(serial, quality)?);
    Ok(())
}

/// Stop the active session. Idempotent: stopping with nothing running is a
/// no-op, mirroring how the UI disables Stop when idle.
pub fn request_stop() -> Result<(), String> {
    if let Some(mut session) = active().take() {
        session.shutdown();
    }
    Ok(())
}

/// Current session status (or the idle default when nothing ever ran).
pub fn poll_status() -> MirrorStatus {
    active().as_ref().map(Session::snapshot).unwrap_or_default()
}

/// Resolve the shipped scrcpy server jar: installed layout (bundled
/// `binaries/` next to the executable) → development tree. Mirrors
/// [`adb::resolve`]'s ordering.
fn server_jar() -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for base in [dir.join("binaries"), dir.to_path_buf()] {
                let path = base.join(SERVER_FILE);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }
    let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(SERVER_FILE);
    if dev.is_file() {
        return Ok(dev);
    }
    Err(format!(
        "scrcpy server not found: {SERVER_FILE} (expected under binaries/)"
    ))
}

/// The remote command line for the scrcpy server (`adb shell` argv).
fn server_args(serial: &str, quality: MirrorQuality) -> Vec<String> {
    let mut args = vec![
        "-s".to_string(),
        serial.to_string(),
        "shell".to_string(),
        format!("CLASSPATH={REMOTE_JAR}"),
        "app_process".to_string(),
        "/".to_string(),
        SERVER_CLASS.to_string(),
        SERVER_VERSION.to_string(),
        // Pure H.264 Annex-B: no device meta, no frame meta, no dummy byte.
        "raw_stream=true".to_string(),
        // Phase 2 scope: video only, no audio pipeline.
        "audio=false".to_string(),
        // Single connection: the one we forward is the video socket, so the
        // server must not block waiting for a control client.
        "control=false".to_string(),
        // Server listens on the abstract socket; we forward to it.
        "tunnel_forward=true".to_string(),
    ];
    if quality.max_size > 0 {
        args.push(format!("max_size={}", quality.max_size));
    }
    if quality.bitrate > 0 {
        args.push(format!("video_bit_rate={}", quality.bitrate));
    }
    if quality.max_fps > 0 {
        args.push(format!("max_fps={}", quality.max_fps));
    }
    args
}

/// Connect to the forwarded port and only hand the socket over once it
/// actually holds: a forward that immediately EOFs means the server has not
/// listened yet (or died), so retry within the handshake budget.
fn wait_for_forward() -> Result<TcpStream, String> {
    let mut last = "not reachable".to_string();
    let mut alive_ticks = 0u32;
    for _ in 0..CONNECT_ATTEMPTS {
        match TcpStream::connect(("127.0.0.1", FORWARD_PORT)) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(CONNECT_INTERVAL));
                let mut probe = [0u8; 1];
                match stream.peek(&mut probe) {
                    Ok(0) => {
                        // Remote side closed: server not listening yet.
                        last = "forward closed (scrcpy server not listening)".to_string();
                        alive_ticks = 0;
                    }
                    Ok(_) => {
                        // Bytes already waiting: definitely live.
                        let _ = stream.set_read_timeout(None);
                        return Ok(stream);
                    }
                    Err(_) => {
                        // Held open but silent (encoder warming up). Accept
                        // once it survives a few ticks without closing.
                        alive_ticks += 1;
                        if alive_ticks >= 3 {
                            let _ = stream.set_read_timeout(None);
                            return Ok(stream);
                        }
                        last = "forward open, waiting for the server".to_string();
                    }
                }
            }
            Err(error) => last = format!("connect failed: {error}"),
        }
        std::thread::sleep(CONNECT_INTERVAL);
    }
    Err(format!("mirror transport did not open: {last}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_args_keep_the_video_only_contract() {
        let args = server_args("SERIAL", MirrorQuality::default());
        let joined = args.join(" ");
        assert!(joined.contains("raw_stream=true"));
        assert!(joined.contains("audio=false"));
        assert!(joined.contains("control=false"));
        assert!(joined.contains("tunnel_forward=true"));
        assert!(joined.contains(SERVER_CLASS));
        assert!(joined.contains(SERVER_VERSION));
        // Defaults must not override scrcpy's own defaults.
        assert!(!joined.contains("max_size="));
        assert!(!joined.contains("video_bit_rate="));
        assert!(!joined.contains("max_fps="));
    }

    #[test]
    fn server_args_forward_quality() {
        let args = server_args(
            "SERIAL",
            MirrorQuality {
                max_size: 1280,
                bitrate: 4_000_000,
                max_fps: 60,
            },
        );
        let joined = args.join(" ");
        assert!(joined.contains("max_size=1280"));
        assert!(joined.contains("video_bit_rate=4000000"));
        assert!(joined.contains("max_fps=60"));
    }

    #[test]
    fn ships_the_scrcpy_server_jar() {
        let jar = server_jar().expect("bundled scrcpy server jar must exist");
        assert!(jar.is_file());
    }
}
