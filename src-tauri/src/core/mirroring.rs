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
//! the socket carries pure H.264 Annex-B; `control=true` makes the server
//! also accept the control socket, which is the second connection to the same
//! forward and carries input events (see [`crate::core::input`]);
//! `tunnel_forward=true` makes the server listen on `localabstract:scrcpy`,
//! which `adb forward` reaches.
//!
//! USB transport only for now. `adb forward` is transport-agnostic, so a
//! future Wi-Fi transport replaces the setup here, not the pipeline.

use crate::core::adb;
use crate::core::audio::{self, AudioStats};
use crate::core::input;
use crate::core::video::{self, PlaybackControl, StreamEnd, VideoStats};
use serde::Serialize;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
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
/// Readiness budget: poll the device until it owns the abstract socket
/// before connecting (~20 s — a cold first start exposes it a few seconds
/// in, and connecting blind used to give up first).
const READINESS_ATTEMPTS: u32 = 40;
const READINESS_INTERVAL: Duration = Duration::from_millis(500);
/// Connect budget: connect, then accept the stream only once the forward
/// actually holds (the server may take a moment to listen). Its own budget,
/// so a socket that appears late still gets connected.
const CONNECT_ATTEMPTS: u32 = 20;
const CONNECT_INTERVAL: Duration = Duration::from_millis(300);
/// How long one accepted connection may stay silent before we decide it is
/// a healthy session. The scrcpy encoder needs ~0.5 s to emit its first
/// bytes (measured: TTFB at +0.5 s, then the whole H.264 backlog), so a 2 s
/// window sits comfortably past warm-up. Distinct from the retry interval:
/// the server tears itself down when its single accepted connection closes,
/// so during this window the socket must be *held*, never closed and
/// retried — only an EOF justifies another attempt.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

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

impl MirrorQuality {
    /// The named modes the Device page offers, mapped onto the scrcpy knobs
    /// above. `sharp` is [`MirrorQuality::default`] — the engine's tested
    /// behaviour — so the default path stays byte-identical; the other modes
    /// trade pixels for bandwidth explicitly.
    pub fn preset(name: &str) -> Result<Self, String> {
        match name {
            "sharp" => Ok(Self::default()),
            "balanced" => Ok(Self {
                max_size: 1280,
                bitrate: 6_000_000,
                max_fps: 60,
            }),
            "smooth" => Ok(Self {
                max_size: 1024,
                bitrate: 4_000_000,
                max_fps: 60,
            }),
            other => Err(format!("unknown mirror mode '{other}'")),
        }
    }
}

/// Session state for the UI, pushed on change via `mirror-changed`.
#[derive(Clone, Debug, PartialEq, Serialize)]
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
    /// Pipeline counters sampled once per second by the running session;
    /// `None` when nothing is publishing them. Stage measures of the local
    /// pipeline only — not input-to-mirror latency (that needs input
    /// injection, Phase 3).
    pub stats: Option<VideoStats>,
    /// Consecutive seconds the decoded picture has carried no visible content
    /// (`video::picture_blank_secs`). `0` while the picture shows anything.
    /// The UI uses it to explain an empty mirror; it is a measurement of the
    /// picture, so it names no cause.
    pub picture_blank_secs: u32,
    /// Audio counters sampled once per second by the running session
    /// (`audio::stats`); `None` while no audio stream is running. `error` is
    /// set when the desktop has no usable output device — the video session
    /// keeps running in that case.
    pub audio: Option<AudioStats>,
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
            stats: None,
            picture_blank_secs: 0,
            audio: None,
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
    audio_thread: Option<std::thread::JoinHandle<()>>,
    audio_stop: Arc<AtomicBool>,
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
            let _ = adb::run(&adb.path, &["forward", "--remove", &forward_arg]);
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
            let _ = adb::run(&adb.path, &["forward", "--remove", &forward_arg]);
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

        let stream = match wait_for_forward(&adb.path, serial) {
            Ok(stream) => stream,
            Err(error) => {
                cleanup();
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };

        // The audio path, opened before anything else touches the forward:
        // the server accepts in a fixed order — video, audio, control — on the
        // same local socket, so a control connection opened first would
        // receive the PCM stream instead of accepting input.
        let audio_stream = match TcpStream::connect(("127.0.0.1", FORWARD_PORT)) {
            Ok(stream) => stream,
            Err(error) => {
                cleanup();
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "audio socket refused on 127.0.0.1:{FORWARD_PORT}: {error}"
                ));
            }
        };
        audio::reset();

        // The input path. The server accepted the video and audio sockets and
        // is now blocked in `accept()` for the control socket, so this
        // connects immediately and the session starts streaming only once it
        // exists.
        let controller = match input::Controller::connect(FORWARD_PORT) {
            Ok(controller) => Arc::new(controller),
            Err(error) => {
                cleanup();
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        input::attach(controller);

        let control = Arc::new(PlaybackControl::default());
        // Audio rides its own thread: it blocks on the socket and on the
        // render device, so it must never hold up the picture. Its stop flag
        // is separate from the decoder's — the socket closing is what ends the
        // stream — and teardown trips it before joining.
        let audio_stop = Arc::new(AtomicBool::new(false));
        let thread_audio_stop = audio_stop.clone();
        let audio_thread = std::thread::Builder::new()
            .name("mirror-audio".to_string())
            .spawn(move || audio::play(audio_stream, thread_audio_stop))
            .map_err(|error| format!("cannot start the audio thread: {error}"));
        let status = Arc::new(Mutex::new(MirrorStatus {
            running: true,
            phase: "starting",
            // The session's own serial, so the UI can tell which device (and
            // which of its transports) this status belongs to. Without it every
            // card but the first looks like it belongs to another device.
            serial: serial.to_string(),
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
                    &["forward", "--remove", &format!("tcp:{FORWARD_PORT}")],
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
                // The stream is gone, so the control socket is too: a window
                // that outlives it must not keep sending input.
                input::detach();
            })
            .map_err(|error| format!("cannot start mirror session: {error}"));
        let thread = match spawned {
            Ok(thread) => thread,
            Err(error) => {
                input::detach();
                cleanup();
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };

        let audio_thread = match audio_thread {
            Ok(thread) => Some(thread),
            Err(error) => {
                audio_stop.store(true, Ordering::SeqCst);
                input::detach();
                let _ = thread.join();
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
            audio_thread,
            audio_stop,
        })
    }

    /// Stop the decode thread, then reap the server process and forward.
    fn shutdown(&mut self) {
        // No new input from the moment the session is going down.
        input::detach();
        self.control.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // The picture is down, so the audio stream is too: trip the flag and
        // wait for the render device to be released before the server dies.
        self.audio_stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.audio_thread.take() {
            let _ = thread.join();
        }
        if let Ok(adb) = adb::resolve() {
            let _ = adb::run(
                &adb.path,
                &["-s", &self.serial, "shell", "pkill", "-f", SERVER_CLASS],
            );
            let _ = adb::run(
                &adb.path,
                &["forward", "--remove", &format!("tcp:{FORWARD_PORT}")],
            );
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn snapshot(&self) -> MirrorStatus {
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        // The counters belong to the session, not the engine: `video` publishes
        // them once per second while it decodes. Never attach one to a status
        // that is not running — a finished session's numbers would read as live.
        status.stats = if status.running { video::stats() } else { None };
        status.picture_blank_secs = if status.running {
            video::picture_blank_secs()
        } else {
            0
        };
        // Same rule for the audio counters: they belong to the session.
        status.audio = if status.running { audio::stats() } else { None };
        status
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
        // The device's own playback, as plain PCM. `audio_codec=raw` is what
        // makes this need no decoder: the socket carries 48 kHz stereo 16-bit
        // samples (scrcpy's `AudioConfig`), and `raw_stream=true` above also
        // removes the audio stream header, so the bytes go straight to the
        // desktop's audio device.
        "audio=true".to_string(),
        "audio_codec=raw".to_string(),
        // Input rides scrcpy's control channel: with control enabled the
        // server accepts a second connection on the same forward, right after
        // the video socket. The client must open it (see `Session::start`),
        // otherwise the server waits in `accept()` and never streams.
        "control=true".to_string(),
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

/// One readiness probe: does the device itself own the abstract socket?
/// Only a listening server makes that entry appear, and only then can the
/// forward possibly hold. `Err` means the probe failed (adb or
/// `/proc/net/unix` unreadable) — never treat that as "not ready".
fn device_socket_ready(adb: &std::path::Path, serial: &str) -> Result<bool, String> {
    let output = adb::run(adb, &["-s", serial, "shell", "cat", "/proc/net/unix"])?;
    let wanted = format!("@{SOCKET_NAME}");
    Ok(output
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .any(|field| field == wanted))
}

/// Two-stage transport wait. Stage 1 polls the device for the abstract
/// socket (readiness, ~20 s); stage 2 connects and probes the forward with
/// its own budget, so a cold first start — socket up seconds in — is no
/// longer killed by a blind 4.5 s deadline.
///
/// Stage 2's invariant: a held-but-silent connection is a healthy session,
/// only an EOF means retry. The scrcpy server handles exactly one
/// connection and tears the whole session down when it closes, so every
/// attempt keeps its accepted stream open for the whole [`PROBE_TIMEOUT`]
/// probe: data already buffered (`Ok(n)`) or silence without closure
/// (`Err` timeout/`WouldBlock`, encoder still warming up) are both returned
/// as the session stream. Only `Ok(0)` — the peer itself closed — or a
/// failed `connect` drops the attempt and tries again.
///
/// An unreadable `/proc/net/unix` must not abort the wait: with no readiness
/// signal the function falls back to plain connect retries for the full
/// readiness budget instead.
fn wait_for_forward(adb: &std::path::Path, serial: &str) -> Result<TcpStream, String> {
    // Stage 1: readiness on the device side.
    let mut probe_failed = false;
    for _ in 0..READINESS_ATTEMPTS {
        match device_socket_ready(adb, serial) {
            Ok(true) => break,
            Ok(false) => {}
            Err(_) => {
                probe_failed = true;
                break;
            }
        }
        std::thread::sleep(READINESS_INTERVAL);
    }

    // Stage 2: connect and probe. Without a readiness signal there is
    // nothing to wait for, so plain-retry for the whole readiness budget.
    let (attempts, interval) = if probe_failed {
        (READINESS_ATTEMPTS, READINESS_INTERVAL)
    } else {
        (CONNECT_ATTEMPTS, CONNECT_INTERVAL)
    };
    let mut last = "not reachable".to_string();
    for _ in 0..attempts {
        // One connect per attempt; the accepted stream is held through the
        // probe below and only dropped on EOF, because closing a connection
        // the server already accepted ends the whole streaming session.
        match TcpStream::connect(("127.0.0.1", FORWARD_PORT)) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(PROBE_TIMEOUT));
                let mut probe = [0u8; 1];
                match stream.peek(&mut probe) {
                    // The peer closed: server not listening yet (or it
                    // died). Drop this connection and retry.
                    Ok(0) => last = "forward closed (scrcpy server not listening)".to_string(),
                    // Bytes already waiting: definitely live.
                    Ok(_) => {
                        let _ = stream.set_read_timeout(None);
                        return Ok(stream);
                    }
                    // Held open but silent: the encoder is still warming
                    // up, which is the normal first ~0.5 s. A healthy
                    // session — take it now rather than closing the socket
                    // and killing the server.
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) =>
                    {
                        let _ = stream.set_read_timeout(None);
                        return Ok(stream);
                    }
                    // Hard read error: the connection is unusable, retry.
                    Err(error) => last = format!("probe failed: {error}"),
                }
            }
            // Retryable hard connect failure (e.g. connection refused).
            Err(error) => last = format!("connect failed: {error}"),
        }
        std::thread::sleep(interval);
    }

    // Honest timeout: one last probe names the stage that ran out of budget.
    let stage = match device_socket_ready(adb, serial) {
        Ok(true) => {
            format!("socket @{SOCKET_NAME} is up but the forward never became connectable ({last})")
        }
        Ok(false) => {
            format!("socket @{SOCKET_NAME} never appeared — scrcpy server not listening ({last})")
        }
        Err(error) => format!(
            "readiness probe unavailable ({error}); forward never became connectable ({last})"
        ),
    };
    Err(format!("mirror transport did not open: {stage}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_args_open_the_control_channel() {
        let args = server_args("SERIAL", MirrorQuality::default());
        let joined = args.join(" ");
        assert!(joined.contains("raw_stream=true"));
        // Audio is forwarded as raw PCM: the device's own playback, no decoder
        // on this side (see `core::audio`).
        assert!(joined.contains("audio=true"));
        assert!(joined.contains("audio_codec=raw"));
        // Pointer input needs the control socket: the client opens the second
        // connection to the same forward.
        assert!(joined.contains("control=true"));
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

    /// The webview reads these keys by name: the counters must survive
    /// serialisation exactly as `src/lib/tauri.ts` declares them.
    #[test]
    fn status_serialises_the_counters_for_the_webview() {
        let status = MirrorStatus {
            running: true,
            phase: "running",
            reason: String::new(),
            width: 720,
            height: 1604,
            serial: "SERIAL".to_string(),
            stats: Some(video::VideoStats {
                fps: 50.4,
                bytes_per_s: 1_062_500.0,
                decode_ms: 16.2,
                blit_ms: 9.1,
                overwritten: 47,
                frames_decoded: 613,
            }),
            picture_blank_secs: 3,
            audio: Some(audio::AudioStats {
                device: "Speakers (Realtek)".to_string(),
                sample_rate: 48_000,
                channels: 2,
                kbps: 1411.2,
                peak: 9_318,
                played_ms: 12_400,
                total_bytes: 2_116_800,
                error: String::new(),
            }),
        };
        let json = serde_json::to_value(&status).expect("status must serialise");
        assert_eq!(json["stats"]["fps"], 50.4);
        assert_eq!(json["stats"]["bytesPerS"], 1_062_500.0);
        assert_eq!(json["stats"]["decodeMs"], 16.2);
        assert_eq!(json["stats"]["blitMs"], 9.1);
        assert_eq!(json["stats"]["overwritten"], 47);
        assert_eq!(json["stats"]["framesDecoded"], 613);
        // The blank-picture run the UI turns into its notice.
        assert_eq!(json["pictureBlankSecs"], 3);
        // No session publishing: the field is present and null, never absent.
        let idle = serde_json::to_value(MirrorStatus::default()).unwrap();
        assert_eq!(idle["stats"], serde_json::Value::Null);
        assert!(idle.get("stats").is_some());
        // An idle status never carries a blank run from a finished session.
        assert_eq!(idle["pictureBlankSecs"], 0);
        // Audio counters reach the webview under the names it declares.
        assert_eq!(json["audio"]["device"], "Speakers (Realtek)");
        assert_eq!(json["audio"]["sampleRate"], 48_000);
        assert_eq!(json["audio"]["channels"], 2);
        assert_eq!(json["audio"]["kbps"], 1411.2);
        assert_eq!(json["audio"]["peak"], 9_318);
        assert_eq!(json["audio"]["playedMs"], 12_400);
        assert_eq!(json["audio"]["totalBytes"], 2_116_800);
        assert_eq!(json["audio"]["error"], "");
        assert_eq!(idle["audio"], serde_json::Value::Null);
    }
}

#[cfg(test)]
mod mirror_mode_tests {
    use super::MirrorQuality;

    /// `sharp` is the shipped default, so selecting it cannot change behaviour;
    /// the other modes only lower the knobs, and an unknown name is rejected
    /// rather than silently accepted.
    #[test]
    fn named_modes_map_onto_the_quality_knobs() {
        assert_eq!(
            MirrorQuality::preset("sharp").unwrap(),
            MirrorQuality::default()
        );
        let smooth = MirrorQuality::preset("smooth").unwrap();
        assert_eq!(smooth.max_size, 1024);
        assert!(smooth.bitrate <= MirrorQuality::preset("balanced").unwrap().bitrate);
        assert!(MirrorQuality::preset("ultra").is_err());
    }
}
