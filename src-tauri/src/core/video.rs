//! Video: decoder and renderer boundary.
//!
//! Receives the raw H.264 byte stream (scrcpy server in `raw_stream=true`
//! mode) from [`crate::core::mirroring`], splits it into access units and
//! decodes it through [`crate::core::h264`] — the Windows Media Foundation
//! H.264 decoder MFT, because a Media Foundation *source reader* cannot open
//! a containerless Annex-B stream — then presents the frames in a native
//! Win32 mirror window. Frame data stays in native code and never crosses
//! the JavaScript boundary.
//!
//! The interface is deliberately transport-agnostic: mirroring hands over an
//! already-connected TCP socket (today via `adb forward`, later possibly
//! Wi-Fi) and this module only sees "H.264 bytes in, frames on screen".
//! Decoding and presentation are independent of React, so a future clean
//! floating mirror window can reuse the same pipeline without decoding twice.
//!
//! Presentation follows the decode side. With a hardware Direct3D 11 adapter
//! the mirror window presents through a DXGI swap chain and the GPU converts
//! and scales the picture ([`crate::core::gpu`]); the window draws with GDI
//! otherwise, and drops back to GDI if presenting ever fails. A frame decoded
//! into a device texture is read back to system memory on its way to GDI, so
//! every combination of decode and presentation works.

use serde::Serialize;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Cross-thread session control flags shared by the engine, the socket byte
/// stream and the mirror window.
#[derive(Default)]
pub struct PlaybackControl {
    /// Engine requested shutdown (stop button or session teardown).
    pub stop: AtomicBool,
    /// The mirror window is gone (closed by the user or by teardown).
    pub window_dead: AtomicBool,
    /// The user closed the mirror window — classify the end as a clean
    /// shutdown rather than a disconnect.
    pub user_closed: AtomicBool,
}

impl PlaybackControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once the session must wind down (stop requested or window gone).
    pub fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::SeqCst) || self.window_dead.load(Ordering::SeqCst)
    }
}

/// Why playback ended. Maps onto the session phase the engine reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEnd {
    /// Engine or caller stopped the session.
    Stopped,
    /// User closed the mirror window.
    UserClosed,
    /// Stream ended without a stop request (device unplugged, server died).
    Disconnected,
    /// Decoder/presentation failure with a user-facing message.
    Failed(String),
}

/// One decoded frame, in whichever form the decode side produced it: a device
/// texture when the decoder ran on the GPU, or system-memory BGRA when it ran
/// on the CPU. It is the decoder's frame type, re-exported because it is what
/// crosses the decode-to-window boundary.
pub use crate::core::h264::Frame;

/// One sample of the running session's pipeline counters (see [`stats`]).
///
/// Every field measures a stage of the local pipeline and is named the way the
/// measurement CSV names it: the CSV row and this struct are both produced by
/// [`VideoStats::from_deltas`], so the two cannot drift apart. None of these
/// numbers is input-to-mirror latency — that path needs input injection
/// (Phase 3), and a frame interval is not a latency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStats {
    /// Frames the decoder handed back in the last sample window, per second.
    pub fps: f64,
    /// H.264 bytes read off the socket in the last window, per second.
    pub bytes_per_s: f64,
    /// Decoder time (decode + NV12->BGRA) spent in the last window, in ms.
    pub decode_ms: f64,
    /// Presentation (blit) time spent in the last window, in ms.
    pub blit_ms: f64,
    /// Frames handed to the presenter in the last window that a newer frame
    /// superseded before they were painted (the CSV's `frames_overwritten`:
    /// delta delivered - delta taken). Not a total "dropped" count.
    pub overwritten: u64,
    /// Frames decoded since this session started.
    pub frames_decoded: u64,
}

impl VideoStats {
    /// Counters -> sample: the single definition of these numbers.
    ///
    /// `now`/`prev` use the counter layout the session thread collects in
    /// `native::play`; `secs` is the length of the sample window.
    fn from_deltas(now: &[u64; 14], prev: &[u64; 14], secs: f64) -> Self {
        let delta = |index: usize| now[index].saturating_sub(prev[index]);
        let secs = if secs > 0.0 { secs } else { f64::MIN_POSITIVE };
        Self {
            fps: delta(3) as f64 / secs,
            bytes_per_s: delta(1) as f64 / secs,
            decode_ms: delta(8) as f64 / 1e6,
            blit_ms: delta(9) as f64 / 1e6,
            overwritten: delta(4).saturating_sub(delta(5)),
            frames_decoded: now[3],
        }
    }
}

/// Latest published sample. Written once per second by the session thread
/// through [`publish`]; `None` while no session is publishing.
static SNAPSHOT: std::sync::Mutex<Option<VideoStats>> = std::sync::Mutex::new(None);

/// Publish (`Some`) or retract (`None`) the running session's sample.
///
/// Retracting also clears the blank-picture run: a session that has not started
/// yet (or has ended) must not leave the previous run standing for the UI to
/// report. The 1 Hz sampler calls this with `Some` on every tick, so it must
/// leave the run alone — it belongs to the very session being sampled.
fn publish(sample: Option<VideoStats>) {
    let retracting = sample.is_none();
    *SNAPSHOT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = sample;
    if retracting {
        note_picture(false);
    }
}

/// When the current run of blank pictures began; `None` while the picture shows
/// anything visible (see [`picture_has_no_content`]).
///
/// Written by the session's window thread, read by the UI. A probe that could
/// not be taken counts as content: a notice must never appear because a
/// measurement failed.
static BLANK_SINCE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Seconds the running session's picture has carried no visible content; `0`
/// while it shows anything at all, or while no session is running.
///
/// Measured as wall time since the run began. Frames arrive slowly for a static
/// or blank picture, so counting probes would under-report how long the user
/// has been looking at nothing.
pub fn picture_blank_secs() -> u32 {
    BLANK_SINCE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .map(|since| since.elapsed().as_secs() as u32)
        .unwrap_or(0)
}

/// Fold one probe into [`BLANK_SINCE`]: the first blank starts the run, any
/// visible content ends it.
fn note_picture(blank: bool) {
    let mut since = BLANK_SINCE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if blank {
        if since.is_none() {
            *since = Some(std::time::Instant::now());
        }
    } else {
        *since = None;
    }
}

/// Mean luminance (0-255) below which a picture counts as carrying no content.
///
/// Measured, not guessed: a protected stream produced a client area of mean
/// 0.63/255 across two captures, while a dark-mode settings screen of the same
/// size measures far above this floor. It is deliberately low so that dimmed
/// video, dark themes and lock screens are never mistaken for blank — and the
/// notice the UI shows for it states the measurement, not a cause.
const CONTENT_LUMA_FLOOR: f64 = 6.0;

/// Pixels sampled per probe: enough to tell "no content" from any real screen,
/// few enough to run every second on the window thread.
const CONTENT_SAMPLES: usize = 4000;

/// Log a content-probe failure once per process.
///
/// The probe can only report a blank picture if it can sample one; a silent
/// failure here would leave the UI notice off forever, which must not be
/// mistaken for "there is content".
fn probe_failed(error: impl std::fmt::Display) {
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!(
            "mirror: the content probe cannot sample the picture ({error}); \
             the blank-picture notice will not appear"
        );
    }
}

/// True when a BGRA picture is (near) uniformly black.
///
/// Samples a bounded number of pixels spread over the whole picture, so the
/// cost does not grow with resolution.
fn picture_has_no_content(bgra: &[u8], stride: usize, width: u32, height: u32) -> bool {
    let (width, height) = (width as usize, height as usize);
    if width == 0 || height == 0 || stride < width * 4 {
        return false;
    }
    let step = (width * height / CONTENT_SAMPLES).max(1);
    let mut total = 0u64;
    let mut taken = 0u64;
    let mut pixel = 0usize;
    while pixel < width * height {
        let at = (pixel / width) * stride + (pixel % width) * 4;
        if at + 2 < bgra.len() {
            total += (bgra[at] as u64 + bgra[at + 1] as u64 + bgra[at + 2] as u64) / 3;
            taken += 1;
        }
        pixel += step;
    }
    taken > 0 && (total as f64 / taken as f64) < CONTENT_LUMA_FLOOR
}

/// Latest one-second sample of the running session's counters.
///
/// This is what the product's diagnostics surface reads; the optional CSV dump
/// (`HERMESGATE_VIDEO_STATS=<path>`) writes the same numbers from the same
/// computation. `None` means no session is running — a finished session's
/// sample is retracted, never left behind for a UI to show as live.
pub fn stats() -> Option<VideoStats> {
    *SNAPSHOT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Decode `stream` and present it until the session ends (blocking; runs for
/// the whole session on the caller's thread).
///
/// `on_size(width, height)` fires when the decoded video size becomes known
/// and whenever it changes (device rotation), so the engine can publish
/// dimensions to the UI.
#[cfg(windows)]
pub fn play(
    stream: Arc<TcpStream>,
    control: Arc<PlaybackControl>,
    title: &str,
    on_size: impl FnMut(u32, u32),
) -> StreamEnd {
    native::play(stream, control, title, on_size)
}

/// Non-Windows builds compile the mirroring engine (for CI and development)
/// but have no native decoder yet.
#[cfg(not(windows))]
pub fn play(
    _stream: Arc<TcpStream>,
    _control: Arc<PlaybackControl>,
    _title: &str,
    _on_size: impl FnMut(u32, u32),
) -> StreamEnd {
    StreamEnd::Failed("mirroring is only supported on Windows".to_string())
}

#[cfg(windows)]
mod native {
    use super::{Frame, PlaybackControl, StreamEnd};
    use crate::core::gpu;
    use crate::core::h264::Decoder;
    use crate::core::input;
    use std::cell::RefCell;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, EndPaint, InvalidateRect, ScreenToClient, SetBrushOrgEx, SetStretchBltMode,
        StretchDIBits, UpdateWindow, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HALFTONE, HBRUSH, HDC, PAINTSTRUCT, SRCCOPY,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Blocking socket reads tick at least this often so Stop and window
    /// close stay responsive while Media Foundation waits for bytes.
    const READ_TICK: Duration = Duration::from_millis(400);

    /// Window class registered once per process.
    const CLASS_NAME: PCWSTR = w!("HermesGateMirror");

    // ----------------------------------------------------------- measurement

    /// Counters for the existing pipeline stages: the measurement instrument
    /// behind the benchmark numbers, and the source of the one-second sample
    /// the UI reads ([`super::stats`]). Always live — a relaxed add per stage
    /// is the whole cost. Writing them to a CSV is what stays opt-in:
    /// `HERMESGATE_VIDEO_STATS` names the file, the session thread is its only
    /// writer, and an unset variable writes nothing.
    #[derive(Default)]
    struct Stats {
        /// Socket read rounds entered.
        rounds: AtomicU64,
        /// H.264 bytes read off the socket.
        bytes_in: AtomicU64,
        /// Nanoseconds blocked in the socket read.
        read_ns: AtomicU64,
        /// Nanoseconds inside `Decoder::push`/`finish` (decode + NV12→BGRA).
        decode_ns: AtomicU64,
        /// Decoder feed calls.
        decode_calls: AtomicU64,
        /// Frames handed back by the decoder.
        decoded: AtomicU64,
        /// Frames written into the presenter slot.
        delivered: AtomicU64,
        /// Frames the presenter took out of the slot (rendered frames).
        taken: AtomicU64,
        /// WM_PAINT passes that drew something.
        painted: AtomicU64,
        /// Nanoseconds inside the GDI blit.
        blit_ns: AtomicU64,
        /// Presenter loop iterations.
        iters: AtomicU64,
        /// Presenter iterations that found no fresh frame.
        empty: AtomicU64,
        /// Nanoseconds spent in the whole presenter loop body.
        loop_ns: AtomicU64,
        /// Nanoseconds inside `paint` from `BeginPaint` to `EndPaint`.
        paint_ns: AtomicU64,
    }

    impl Stats {
        /// `Some` when the CSV path is configured and creatable: the opt-in
        /// dump of the counters the UI also samples.
        fn open_file() -> Option<std::fs::File> {
            let path = std::env::var_os("HERMESGATE_VIDEO_STATS")?;
            if path.is_empty() {
                return None;
            }
            let mut file = std::fs::File::create(std::path::PathBuf::from(path)).ok()?;
            let _ = writeln!(
                file,
                "t_s,rounds,bytes_in,bytes_per_s,decode_calls,frames_decoded,fps_decoded,\
                 frames_delivered,frames_taken,frames_painted,frames_overwritten,\
                 read_ms,decode_ms,blit_ms,read_pct,decode_pct,iters,empty,loop_ms,paint_ms"
            );
            Some(file)
        }

        fn add(counter: &AtomicU64, value: u64) {
            counter.fetch_add(value, Ordering::Relaxed);
        }

        fn get(counter: &AtomicU64) -> u64 {
            counter.load(Ordering::Relaxed)
        }
    }

    // ---------------------------------------------------------------- socket

    /// Bytes offered to the decoder per push. The read loop fills this much
    /// or flushes what it has once the socket goes quiet, so whole access
    /// units reach the decoder instead of byte-sized fragments.
    const READ_CHUNK: usize = 32 * 1024;

    /// How one socket read round ended.
    enum ReadOutcome {
        /// `n` bytes are ready for the decoder.
        Data(usize),
        /// Nothing arrived before the read tick: the session is still alive.
        Idle,
        /// The server closed the stream cleanly.
        Closed,
        /// The socket failed (device unplugged, `adb forward` lost).
        Lost,
    }

    /// Read one round off the device socket, ticking every `READ_TICK` so
    /// Stop and mirror-window close stay responsive while the stream is
    /// quiet.
    fn read_round(stream: &TcpStream, chunk: &mut [u8]) -> ReadOutcome {
        let mut source: &TcpStream = stream;
        let mut filled = 0usize;
        loop {
            if filled == chunk.len() {
                return ReadOutcome::Data(filled);
            }
            match source.read(&mut chunk[filled..]) {
                // End of stream: hand the partial round over first, so the
                // tail is decoded rather than dropped.
                Ok(0) => return ended(filled, ReadOutcome::Closed),
                Ok(read) => filled += read,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return ended(filled, ReadOutcome::Idle)
                }
                Err(_) => return ended(filled, ReadOutcome::Lost),
            }
        }
    }

    /// Classify a read round that stopped early: data already read wins over
    /// the reason it stopped.
    fn ended(filled: usize, reason: ReadOutcome) -> ReadOutcome {
        if filled > 0 {
            ReadOutcome::Data(filled)
        } else {
            reason
        }
    }

    // -------------------------------------------------------------- decoder

    // Decoding lives in [`crate::core::h264`]. A Media Foundation source
    // reader cannot open this stream — a containerless Annex-B byte stream
    // has no byte-stream handler, and the resolver answers `0xC00D36C4` —
    // while the H.264 decoder MFT consumes it and hands back frames, either
    // as device textures (when it took the device) or as system-memory BGRA.

    /// The decoder for this session: bound to `device` when there is one, so
    /// decoded pictures stay on the GPU, and in system memory otherwise. A
    /// decoder that cannot be built against the device falls back instead of
    /// failing the session.
    fn build_decoder(
        device: &Option<Arc<gpu::Device>>,
    ) -> Result<Decoder, crate::core::h264::H264Error> {
        let Some(device) = device else {
            return Decoder::new();
        };
        // Decoding on the device used to be opt-in: the Media Foundation
        // H.264 decoder accepted the Direct3D 11 device manager and then never
        // returned from its first `ProcessOutput`. That deadlock turned out to
        // be a caller-side ordering bug — the stream-change notifications have
        // to lead the first sample (`docs/adr/0003`) — and the device path now
        // runs, so it is the default. `HERMESGATE_H264_HW=0` keeps decoding in
        // system memory, which is also where a device path that fails lands.
        if std::env::var("HERMESGATE_H264_HW").is_ok_and(|value| value == "0") {
            return Decoder::new();
        }
        match Decoder::new_hardware(device.clone()) {
            Ok(decoder) => Ok(decoder),
            Err(error) => {
                eprintln!("mirror: decoding on the CPU ({error})");
                Decoder::new()
            }
        }
    }

    // ------------------------------------------------------------- presenter

    /// Owns the native mirror window thread.
    struct Presenter {
        thread: std::thread::JoinHandle<()>,
    }

    impl Presenter {
        fn spawn(
            slot: Arc<Mutex<Option<Frame>>>,
            control: Arc<PlaybackControl>,
            title: &str,
            size: (u32, u32),
            coded: (u32, u32),
            device: Option<Arc<gpu::Device>>,
            stats: Arc<Stats>,
        ) -> Result<Presenter, String> {
            let title = title.to_string();
            std::thread::Builder::new()
                .name("mirror-window".to_string())
                .spawn(move || window_loop(slot, control, &title, size, coded, device, stats))
                .map(|thread| Presenter { thread })
                .map_err(|error| format!("cannot start mirror window: {error}"))
        }

        fn join(self) {
            let _ = self.thread.join();
        }
    }

    /// Hand one decoded frame to the mirror window: the first frame spawns
    /// it, and a changed geometry (device rotation) republishes the size.
    #[allow(clippy::too_many_arguments)]
    fn deliver(
        frame: Frame,
        slot: &Arc<Mutex<Option<Frame>>>,
        presenter: &mut Option<Presenter>,
        size: &mut Option<(u32, u32)>,
        device: &Option<Arc<gpu::Device>>,
        control: &Arc<PlaybackControl>,
        title: &str,
        stats: &Arc<Stats>,
        on_size: &mut impl FnMut(u32, u32),
    ) -> Result<(), String> {
        let frame_size = frame.size();
        if *size != Some(frame_size) {
            *size = Some(frame_size);
            on_size(frame_size.0, frame_size.1);
            if presenter.is_none() {
                *presenter = Some(Presenter::spawn(
                    slot.clone(),
                    control.clone(),
                    title,
                    frame_size,
                    frame.coded(),
                    device.clone(),
                    stats.clone(),
                )?);
            }
        }
        if let Ok(mut latest) = slot.lock() {
            *latest = Some(frame);
        }
        Stats::add(&stats.delivered, 1);
        Ok(())
    }

    /// Per-window thread state, accessed from the window procedure.
    struct WindowState {
        control: Option<Arc<PlaybackControl>>,
        /// The newest frame for `WM_PAINT`; only used while the window draws
        /// with GDI.
        latest: Option<Frame>,
        /// A client area the user resized the window to, waiting for the
        /// window thread to resize the swap chain.
        resize: Option<(u32, u32)>,
        /// The picture the client area shows, in device pixels. Input
        /// coordinates are relative to it (`crate::core::input::map`).
        picture: (u32, u32),
        /// Measurement counters (see [`Stats`]): always live, and sampled
        /// once per second for the UI by the session thread.
        stats: Arc<Stats>,
    }

    /// How the mirror window shows frames.
    ///
    /// With a Direct3D device the window presents through a DXGI swap chain,
    /// which the GPU fills from a decoded texture directly. Without one — or
    /// after presenting fails — the window draws with GDI and frames have to
    /// be in system memory.
    struct Presentation {
        device: Option<Arc<gpu::Device>>,
        presenter: Option<gpu::Presenter>,
    }

    impl Presentation {
        /// Build the presentation for a window and a picture geometry.
        ///
        /// `geometry` is the visible picture, `coded` the extent of the decoded
        /// surface it sits in: the converter is built for the surface, the
        /// swap chain for the window.
        fn new(
            device: Option<Arc<gpu::Device>>,
            hwnd: HWND,
            geometry: (u32, u32),
            coded: (u32, u32),
        ) -> Self {
            let client = client_size(hwnd);
            let presenter = device.as_ref().and_then(|device| {
                match gpu::Presenter::new(device.clone(), hwnd, client, coded, geometry) {
                    Ok(presenter) => Some(presenter),
                    Err(error) => {
                        eprintln!("mirror: drawing with GDI, no GPU presentation: {error}");
                        None
                    }
                }
            });
            Self { device, presenter }
        }

        /// Show one frame.
        ///
        /// `None` means the window presented it; `Some(frame)` means there is
        /// no GPU presentation and the frame belongs to `WM_PAINT` instead.
        fn show(&mut self, frame: Frame) -> Option<Frame> {
            if let Some(presenter) = self.presenter.as_mut() {
                let shown = match &frame {
                    Frame::Texture { texture, .. } => {
                        presenter.present(&texture.texture, texture.subresource)
                    }
                    Frame::Bgra {
                        width,
                        height,
                        bgra,
                        ..
                    } => presenter.present_bgra(bgra, (*width, *height)),
                };
                match shown {
                    Ok(()) => return None,
                    Err(error) => {
                        // A swap chain that failed on this window has to go:
                        // it owns the window's pixels while it exists.
                        eprintln!("mirror: GPU presentation failed, drawing with GDI: {error}");
                        self.presenter = None;
                    }
                }
            }
            match frame {
                Frame::Bgra { .. } => Some(frame),
                Frame::Texture {
                    texture,
                    coded,
                    visible,
                } => {
                    // GDI draws from system memory, so a decoded texture is
                    // read back on the way to the window.
                    let device = self.device.as_ref()?;
                    match device.read_bgra(&texture.texture, texture.subresource, coded, visible) {
                        Ok(bgra) => Some(Frame::Bgra {
                            width: visible.0,
                            height: visible.1,
                            stride: visible.0 * 4,
                            bgra,
                        }),
                        Err(error) => {
                            eprintln!("mirror: reading a decoded picture back failed: {error}");
                            None
                        }
                    }
                }
            }
        }

        /// Does this frame carry any visible content? `None` when it could not
        /// be sampled at all — a texture that will not read back, or no GPU
        /// device to read it with.
        ///
        /// Called at most once a second, so the readback for a decoded texture
        /// costs one picture copy per second.
        fn frame_has_content(&self, frame: &Frame) -> Option<bool> {
            match frame {
                Frame::Bgra {
                    width,
                    height,
                    stride,
                    bgra,
                } => Some(!super::picture_has_no_content(
                    bgra,
                    *stride as usize,
                    *width,
                    *height,
                )),
                Frame::Texture {
                    texture,
                    coded,
                    visible,
                } => {
                    let device = self.device.as_ref()?;
                    match device.read_bgra(&texture.texture, texture.subresource, *coded, *visible)
                    {
                        Ok(bgra) => Some(!super::picture_has_no_content(
                            &bgra,
                            visible.0 as usize * 4,
                            visible.0,
                            visible.1,
                        )),
                        Err(error) => {
                            super::probe_failed(error);
                            None
                        }
                    }
                }
            }
        }

        /// Follow a client area change.
        fn resize(&mut self, client: (u32, u32)) -> Result<(), String> {
            match self.presenter.as_mut() {
                Some(presenter) => presenter.resize(client),
                None => Ok(()),
            }
        }
    }

    thread_local! {
        static STATE: RefCell<Option<WindowState>> = const { RefCell::new(None) };
    }

    fn window_loop(
        slot: Arc<Mutex<Option<Frame>>>,
        control: Arc<PlaybackControl>,
        title: &str,
        initial_size: (u32, u32),
        initial_coded: (u32, u32),
        device: Option<Arc<gpu::Device>>,
        stats: Arc<Stats>,
    ) {
        STATE.with(|state| {
            *state.borrow_mut() = Some(WindowState {
                control: Some(control.clone()),
                latest: None,
                resize: None,
                picture: initial_size,
                stats: stats.clone(),
            });
        });

        let hinstance = match unsafe { GetModuleHandleW(None) } {
            Ok(handle) => handle,
            Err(_) => {
                control.window_dead.store(true, Ordering::SeqCst);
                return;
            }
        };
        let hinstance: HINSTANCE = hinstance.into();
        register_class(hinstance);
        let hwnd = match create_window(hinstance, title, initial_size) {
            Some(hwnd) => hwnd,
            None => {
                control.window_dead.store(true, Ordering::SeqCst);
                return;
            }
        };

        // A window is created hidden: without this the session runs with no
        // visible mirror at all.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
        }

        // The swap chain is built once the window exists: it is sized from
        // the window's client area, which only the window itself knows.
        let mut presentation = Presentation::new(device.clone(), hwnd, initial_size, initial_coded);
        let mut size = initial_size;
        let mut coded = initial_coded;
        // The content probe runs on its own one-second clock: frames arrive at
        // stream rate, but "has the picture been blank for a while" is a
        // per-second question.
        let mut probed = Instant::now();
        'outer: loop {
            let iteration = Instant::now();
            Stats::add(&stats.iters, 1);
            let mut message = MSG::default();
            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                if message.message == WM_QUIT {
                    break 'outer;
                }
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            if control.stop.load(Ordering::SeqCst) {
                let _ = unsafe { DestroyWindow(hwnd) };
            }
            if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                break;
            }
            // The user may have resized the window: the swap chain follows.
            let resized = STATE.with(|state| {
                state
                    .borrow_mut()
                    .as_mut()
                    .and_then(|window| window.resize.take())
            });
            if let Some(client) = resized {
                if let Err(error) = presentation.resize(client) {
                    eprintln!("mirror: resizing the GPU presentation failed: {error}");
                }
            }
            let fresh = slot.lock().map(|mut frame| frame.take()).unwrap_or(None);
            if let Some(frame) = fresh {
                Stats::add(&stats.taken, 1);
                // Once a second, measure whether the picture we are about to
                // show carries any visible content. This is evidence about the
                // stream, not a diagnosis: frames and counters keep climbing
                // whether or not the content is there.
                if probed.elapsed() >= Duration::from_secs(1) {
                    probed = Instant::now();
                    if let Some(content) = presentation.frame_has_content(&frame) {
                        let blank = !content;
                        // Log the start of a blank run, not every second of it.
                        let starting = blank && super::picture_blank_secs() == 0;
                        super::note_picture(blank);
                        if starting {
                            eprintln!(
                                "mirror: the picture carries no visible content; the UI notice \
                                 appears if it stays blank"
                            );
                        }
                    }
                }
                // A rotated stream gets a new window size and a fresh swap
                // chain: surfaces and video processor are sized for the
                // picture they were built for.
                let frame_size = frame.size();
                let frame_coded = frame.coded();
                if frame_size != size || frame_coded != coded {
                    size = frame_size;
                    coded = frame_coded;
                    resize_client(hwnd, size);
                    presentation = Presentation::new(device.clone(), hwnd, size, coded);
                    // Input coordinates are relative to the picture the user
                    // is looking at, which just changed.
                    STATE.with(|state| {
                        if let Some(window) = state.borrow_mut().as_mut() {
                            window.picture = size;
                        }
                    });
                }
                let started = Instant::now();
                if let Some(frame) = presentation.show(frame) {
                    STATE.with(|state| {
                        if let Some(window) = state.borrow_mut().as_mut() {
                            window.latest = Some(frame);
                        }
                    });
                    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
                }
                Stats::add(&stats.blit_ns, started.elapsed().as_nanos() as u64);
            } else {
                Stats::add(&stats.empty, 1);
                std::thread::sleep(Duration::from_millis(6));
            }
            Stats::add(&stats.loop_ns, iteration.elapsed().as_nanos() as u64);
        }

        STATE.with(|state| *state.borrow_mut() = None);
    }

    /// The window's client area in pixels.
    fn client_size(hwnd: HWND) -> (u32, u32) {
        let mut client = RECT::default();
        if unsafe { GetClientRect(hwnd, &mut client) }.is_err() {
            return (1, 1);
        }
        (
            (client.right - client.left).max(1) as u32,
            (client.bottom - client.top).max(1) as u32,
        )
    }

    /// Desktop size in pixels (`SM_CXSCREEN`, `SM_CYSCREEN`).
    fn screen() -> (i32, i32) {
        let width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(320);
        let height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(240);
        (width, height)
    }

    /// Largest client area that still leaves the mirror window usable,
    /// keeping the frame's aspect ratio. A tall phone stream (720x1604) is
    /// scaled down instead of being clamped off the bottom of a 1080p
    /// desktop; the blit stretches it back to whatever client area results.
    fn fit_to_screen(size: (u32, u32)) -> (u32, u32) {
        let (frame_w, frame_h) = (size.0.max(1), size.1.max(1));
        let (screen_w, screen_h) = screen();
        // Leave room for the title bar and the taskbar.
        let max_w = (screen_w as u32 * 9 / 10).max(320);
        let max_h = (screen_h as u32 * 85 / 100).max(240);
        if frame_w <= max_w && frame_h <= max_h {
            return (frame_w, frame_h);
        }
        let scale = f64::min(max_w as f64 / frame_w as f64, max_h as f64 / frame_h as f64);
        (
            ((frame_w as f64 * scale).round() as u32).max(1),
            ((frame_h as f64 * scale).round() as u32).max(1),
        )
    }

    fn register_class(hinstance: HINSTANCE) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: WNDCLASS_STYLES(0),
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: HICON(std::ptr::null_mut()),
                hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                lpszMenuName: windows::core::PCWSTR(std::ptr::null()),
                lpszClassName: CLASS_NAME,
                hIconSm: HICON(std::ptr::null_mut()),
            };
            unsafe {
                RegisterClassExW(&class);
            }
        });
    }

    fn create_window(hinstance: HINSTANCE, title: &str, size: (u32, u32)) -> Option<HWND> {
        let size = fit_to_screen(size);
        // A normal overlapped window: the OS draws the title bar the user drags
        // to move the mirror, and the caption boxes whose messages
        // DefWindowProc already routes to this window's WM_CLOSE / WM_SIZE /
        // WM_SYSCOMMAND arms. Nothing is drawn over the client area, so OBS
        // window capture still sees only the mirrored content.
        let (client_w, client_h) = size;
        // The requested size is the *client* size: the picture keeps every pixel
        // the device sent instead of being shrunk by the frame the OS adds.
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: client_w as i32,
            bottom: client_h as i32,
        };
        let _ = unsafe {
            AdjustWindowRectEx(&mut frame, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0))
        };
        let (window_w, window_h) = (frame.right - frame.left, frame.bottom - frame.top);
        // Position: upper-left corner (0,0); the user moves it where they want.
        let x = 0i32;
        let y = 0i32;
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW, // caption + resize frame: a normal desktop window
                x,
                y,
                window_w,
                window_h,
                None,
                None,
                Some(hinstance),
                None,
            )
        };
        created.ok()
    }

    fn resize_client(hwnd: HWND, size: (u32, u32)) {
        let size = fit_to_screen(size);
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: size.0 as i32,
            bottom: size.1 as i32,
        };
        let _ = unsafe {
            AdjustWindowRectEx(&mut rect, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0))
        };
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1), // painted in WM_PAINT — no flicker
            WM_SIZE => {
                // Recorded for the window thread: a swap chain cannot resize
                // itself from inside a message.
                let mut client = RECT::default();
                if unsafe { GetClientRect(hwnd, &mut client) }.is_ok() {
                    let size = (
                        (client.right - client.left).max(1) as u32,
                        (client.bottom - client.top).max(1) as u32,
                    );
                    STATE.with(|state| {
                        if let Some(window) = state.borrow_mut().as_mut() {
                            window.resize = Some(size);
                        }
                    });
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                // User-initiated close: classify the session end as clean.
                STATE.with(|state| {
                    if let Some(window) = state.borrow().as_ref().and_then(|w| w.control.as_ref()) {
                        window.user_closed.store(true, Ordering::SeqCst);
                    }
                });
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            // Pointer input goes to the device while a session has a control
            // channel (Phase 3 scope: left button and wheel; gestures,
            // keyboard and text are not implemented).
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MOUSEMOVE | WM_MOUSEWHEEL => {
                forward_input(hwnd, message, wparam, lparam);
                LRESULT(0)
            }
            WM_DESTROY => {
                STATE.with(|state| {
                    if let Some(window) = state.borrow().as_ref().and_then(|w| w.control.as_ref()) {
                        window.window_dead.store(true, Ordering::SeqCst);
                    }
                });
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    /// Forward one pointer message to the device over the session's control
    /// socket, if there is one.
    ///
    /// The client area shows the whole picture, so window coordinates map
    /// linearly onto device coordinates; a drag that leaves the window while
    /// the pointer is captured stays clamped to the picture.
    fn forward_input(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) {
        let Some(controller) = input::active() else {
            return;
        };
        let picture = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|window| window.picture)
                .filter(|picture| picture.0 > 0 && picture.1 > 0)
        });
        let Some(picture) = picture else {
            return;
        };
        let client = client_size(hwnd);

        if message == WM_MOUSEWHEEL {
            // A wheel message carries screen coordinates, not client ones.
            let mut point = POINT {
                x: signed_loword(lparam),
                y: signed_hiword(lparam),
            };
            if !unsafe { ScreenToClient(hwnd, &mut point) }.as_bool() {
                return;
            }
            let (x, y) = input::map((point.x, point.y), client, picture);
            // Same sign convention as scrcpy's client (`vscroll = event->y`):
            // wheel up (positive delta) is a positive vscroll, which Android
            // answers by moving the view up the list. Measured on device:
            // a negated value scrolled the Settings list the wrong way.
            let notches = input::wheel(((wparam.0 >> 16) as u16 as i16) as i32);
            controller.scroll(x, y, picture, 0.0, notches);
            return;
        }

        let (x, y) = input::map(
            (signed_loword(lparam), signed_hiword(lparam)),
            client,
            picture,
        );
        match message {
            WM_LBUTTONDOWN => {
                // Capture: dragging past the window edge keeps the pointer.
                unsafe { SetCapture(hwnd) };
                controller.touch(input::ACTION_DOWN, x, y, picture, input::BUTTON_PRIMARY);
            }
            WM_MOUSEMOVE if input::dragging(wparam.0) => {
                controller.touch(input::ACTION_MOVE, x, y, picture, input::BUTTON_PRIMARY);
            }
            WM_LBUTTONUP => {
                let _ = unsafe { ReleaseCapture() };
                controller.touch(input::ACTION_UP, x, y, picture, input::BUTTON_NONE);
            }
            _ => {}
        }
    }

    /// `GET_X_LPARAM`: the low word of a message parameter, sign extended.
    fn signed_loword(value: LPARAM) -> i32 {
        (value.0 as u16 as i16) as i32
    }

    /// `GET_Y_LPARAM`: the high word of a message parameter, sign extended.
    fn signed_hiword(value: LPARAM) -> i32 {
        ((value.0 >> 16) as u16 as i16) as i32
    }

    fn paint(hwnd: HWND) {
        let entered = Instant::now();
        let mut paint_struct = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &mut paint_struct) };
        let mut client = RECT::default();
        let _ = unsafe { GetClientRect(hwnd, &mut client) };
        STATE.with(|state| {
            if let Some(window) = state.borrow().as_ref() {
                if let Some(frame) = window.latest.as_ref() {
                    let started = Instant::now();
                    blit(hdc, frame, &client);
                    Stats::add(&window.stats.painted, 1);
                    Stats::add(&window.stats.blit_ns, started.elapsed().as_nanos() as u64);
                }
            }
        });
        let _ = unsafe { EndPaint(hwnd, &paint_struct) };
        STATE.with(|state| {
            if let Some(window) = state.borrow().as_ref() {
                Stats::add(&window.stats.paint_ns, entered.elapsed().as_nanos() as u64);
            }
        });
    }

    /// Draw the frame stretched into the client area (top-down BGRA → DIB).
    ///
    /// Only system-memory frames reach here: a device texture is read back by
    /// [`Presentation::show`] before it is handed to `WM_PAINT`.
    fn blit(hdc: HDC, frame: &Frame, client: &RECT) {
        let Frame::Bgra {
            width,
            height,
            stride,
            bgra,
        } = frame
        else {
            return;
        };
        let (width, height, stride) = (*width, *height, *stride);
        let dest_w = client.right - client.left;
        let dest_h = client.bottom - client.top;
        if dest_w <= 0 || dest_h <= 0 {
            return;
        }
        // The DIB describes the rows as they are laid out, padding included;
        // the source rectangle picks the visible columns out of them.
        let row_bytes = (stride / 4 * 4).max(width * 4);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: (row_bytes / 4) as i32,
                biHeight: -(height as i32), // negative = top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            ..Default::default()
        };
        let _ = unsafe {
            // Halftone: GDI averages the source rows and columns it drops when
            // it shrinks a picture. The default mode deletes them outright,
            // which breaks strokes one pixel wide into dashes — small text is
            // the first thing to go. The brush origin has to be reset with it.
            SetStretchBltMode(hdc, HALFTONE);
            let _ = SetBrushOrgEx(hdc, 0, 0, None);
            StretchDIBits(
                hdc,
                0,
                0,
                dest_w,
                dest_h,
                0,
                0,
                width as i32,
                height as i32,
                Some(bgra.as_ptr().cast()),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            )
        };
    }

    // ------------------------------------------------------------------ play

    /// Map an end-of-stream onto the control flags (stop / close / gone).
    fn control_end(control: &PlaybackControl) -> StreamEnd {
        if control.stop.load(Ordering::SeqCst) {
            StreamEnd::Stopped
        } else if control.user_closed.load(Ordering::SeqCst)
            || control.window_dead.load(Ordering::SeqCst)
        {
            StreamEnd::UserClosed
        } else {
            StreamEnd::Disconnected
        }
    }

    /// A failure, unless the session was already winding down.
    fn or_failure(control: &PlaybackControl, message: String) -> StreamEnd {
        match control_end(control) {
            StreamEnd::Disconnected => StreamEnd::Failed(message),
            other => other,
        }
    }

    pub fn play(
        stream: Arc<TcpStream>,
        control: Arc<PlaybackControl>,
        title: &str,
        mut on_size: impl FnMut(u32, u32),
    ) -> StreamEnd {
        // The socket must tick instead of blocking forever, otherwise Stop
        // would wait for the next video packet.
        let _ = stream.set_read_timeout(Some(READ_TICK));
        let _ = stream.set_nodelay(true);

        // One Direct3D 11 device for the whole pipeline: the decoder writes
        // pictures into its textures, the mirror window presents them through
        // its swap chain. A software rasteriser is no better than the CPU
        // path, so it is not used at all.
        // The Direct3D 11 path is the default: measured on this host it halves
        // the CPU the mirror costs, because the picture never leaves the GPU.
        // `HERMESGATE_VIDEO_GPU=0` pins the pipeline to the software decoder
        // plus the GDI presenter.
        let device = if std::env::var("HERMESGATE_VIDEO_GPU").is_ok_and(|value| value == "0") {
            None
        } else {
            match gpu::Device::new() {
                Ok(device) if device.is_hardware() => {
                    eprintln!(
                        "mirror: {} for GPU decode and presentation",
                        device.adapter()
                    );
                    Some(Arc::new(device))
                }
                Ok(device) => {
                    eprintln!(
                        "mirror: decoding and drawing on the CPU (no hardware Direct3D adapter, {} is software)",
                        device.adapter()
                    );
                    None
                }
                Err(error) => {
                    eprintln!("mirror: decoding and drawing on the CPU ({error})");
                    None
                }
            }
        };

        let mut decoder = match build_decoder(&device) {
            Ok(decoder) => decoder,
            Err(error) => {
                return StreamEnd::Failed(format!("video decoder init failed: {error}"));
            }
        };

        let slot: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
        let mut presenter: Option<Presenter> = None;
        let mut size: Option<(u32, u32)> = None;
        let mut chunk = vec![0u8; READ_CHUNK];

        // Counters are always live: the UI samples them once per second
        // through [`super::stats`]. Only the CSV row is opt-in
        // (`HERMESGATE_VIDEO_STATS=<path>`, see [`Stats`]).
        let stats = Arc::new(Stats::default());
        let mut csv = Stats::open_file();
        // Nothing is sampled yet: retract the previous session's sample so a
        // UI never reads finished numbers as live.
        super::publish(None);
        let started = Instant::now();
        // The decoder only knows which stage it decoded on once it has seen
        // the stream, so the pipeline reports it on the first frame.
        let mut logged_mode = false;
        let mut window = Instant::now();
        let mut prev = [0u64; 14]; // rounds, bytes, calls, decoded, delivered, taken, painted, read_ns, decode_ns, blit_ns, iters, empty, loop_ns, paint_ns

        let end;
        loop {
            if control.stop_requested() {
                end = control_end(&control);
                break;
            }

            let read_start = Instant::now();
            let outcome = read_round(&stream, &mut chunk);
            Stats::add(&stats.read_ns, read_start.elapsed().as_nanos() as u64);
            Stats::add(&stats.rounds, 1);
            let (filled, last) = match outcome {
                ReadOutcome::Data(filled) => {
                    Stats::add(&stats.bytes_in, filled as u64);
                    (filled, false)
                }
                ReadOutcome::Idle => continue, // nothing yet: re-check Stop
                ReadOutcome::Closed => (0, true),
                ReadOutcome::Lost => (0, true),
            };

            // The final round drains the decoder: the bytes already off the
            // wire may complete a frame it is still holding.
            let decode_start = Instant::now();
            let decoded = if last {
                decoder.finish()
            } else {
                decoder.push(&chunk[..filled])
            };
            Stats::add(&stats.decode_ns, decode_start.elapsed().as_nanos() as u64);
            Stats::add(&stats.decode_calls, 1);
            let frames = match decoded {
                Ok(frames) => frames,
                Err(error) => {
                    end = or_failure(&control, format!("video decode failed: {error}"));
                    break;
                }
            };

            // The decode path is only known once the output type is
            // negotiated, which happens after the first access unit. The first
            // decode call usually carries no complete access unit yet, so
            // reporting the path there named the initial value (system memory)
            // even though the same decoder went on to decode on the device
            // (`d-awake.err`, 2026-10-11: "decoding on the CPU" printed between
            // "accepted the Direct3D 11 device manager" and "output type
            // negotiated ((1920, 1080), hardware=true)"). Wait for a frame: a
            // frame cannot come out before the output type is set.
            if !logged_mode && !frames.is_empty() {
                logged_mode = true;
                let stage = if decoder.is_hardware() {
                    "on the GPU"
                } else {
                    "on the CPU"
                };
                eprintln!("mirror: H.264 decoding {stage}");
            }

            let mut failure = None;
            for frame in frames {
                Stats::add(&stats.decoded, 1);
                if let Err(error) = deliver(
                    frame,
                    &slot,
                    &mut presenter,
                    &mut size,
                    &device,
                    &control,
                    title,
                    &stats,
                    &mut on_size,
                ) {
                    failure = Some(error);
                    break;
                }
            }
            if let Some(error) = failure {
                end = StreamEnd::Failed(error);
                break;
            }

            // Once per second: sample the counters (published for the status
            // channel) and write the CSV row when one was requested.
            let elapsed = window.elapsed();
            if elapsed >= Duration::from_secs(1) {
                let secs = elapsed.as_secs_f64();
                let now = [
                    Stats::get(&stats.rounds),
                    Stats::get(&stats.bytes_in),
                    Stats::get(&stats.decode_calls),
                    Stats::get(&stats.decoded),
                    Stats::get(&stats.delivered),
                    Stats::get(&stats.taken),
                    Stats::get(&stats.painted),
                    Stats::get(&stats.read_ns),
                    Stats::get(&stats.decode_ns),
                    Stats::get(&stats.blit_ns),
                    Stats::get(&stats.iters),
                    Stats::get(&stats.empty),
                    Stats::get(&stats.loop_ns),
                    Stats::get(&stats.paint_ns),
                ];
                let delta = |index: usize| now[index].saturating_sub(prev[index]);
                // The single definition: CSV row and status channel both
                // come from this sample.
                let sample = super::VideoStats::from_deltas(&now, &prev, secs);
                super::publish(Some(sample));
                if let Some(file) = csv.as_mut() {
                    let _ = writeln!(
                        file,
                        "{:.1},{},{},{:.0},{},{},{:.1},{},{},{},{},{:.1},{:.1},{:.1},{:.0},{:.0},\
                         {},{},{:.1},{:.1}",
                        started.elapsed().as_secs_f64(),
                        now[0],
                        now[1],
                        sample.bytes_per_s,
                        now[2],
                        now[3],
                        sample.fps,
                        now[4],
                        now[5],
                        now[6],
                        sample.overwritten,
                        delta(7) as f64 / 1e6,
                        sample.decode_ms,
                        sample.blit_ms,
                        delta(7) as f64 / (secs * 1e9) * 100.0,
                        delta(8) as f64 / (secs * 1e9) * 100.0,
                        delta(10),
                        delta(11),
                        delta(12) as f64 / 1e6,
                        delta(13) as f64 / 1e6
                    );
                    let _ = file.flush();
                }
                prev = now;
                window = Instant::now();
            }

            if last {
                end = control_end(&control);
                break;
            }
        }

        // Teardown order: window first, then the decoder (which shuts Media
        // Foundation down).
        control.stop.store(true, Ordering::SeqCst);
        if let Some(presenter) = presenter {
            presenter.join();
        }
        // The session is over: nothing is being sampled any more.
        super::publish(None);
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counter layout `native::play` collects.
    const ZERO: [u64; 14] = [0; 14];

    fn counters(pairs: &[(usize, u64)]) -> [u64; 14] {
        let mut out = ZERO;
        for &(index, value) in pairs {
            out[index] = value;
        }
        out
    }

    /// The content probe behind the UI's "nothing to display" notice: measured
    /// picture statistics, not a guess about the cause.
    #[test]
    fn content_probe_tells_a_blank_picture_from_a_dim_one() {
        let (width, height) = (100u32, 100u32);
        let stride = width as usize * 4;
        let pixels = (width * height) as usize;

        let blank = vec![0u8; stride * height as usize];
        assert!(picture_has_no_content(&blank, stride, width, height));
        // Near-black is what a protected stream measured (0.63/255).
        let near_black = vec![2u8; stride * height as usize];
        assert!(picture_has_no_content(&near_black, stride, width, height));

        // A dark-mode screen is dim, not blank.
        let dim = vec![40u8; stride * height as usize];
        assert!(!picture_has_no_content(&dim, stride, width, height));

        // Sparse bright pixels (text on black) are content too.
        let mut speckled = vec![0u8; stride * height as usize];
        for pixel in (0..pixels).step_by(10) {
            let at = (pixel / width as usize) * stride + (pixel % width as usize) * 4;
            speckled[at..at + 3].fill(255);
        }
        assert!(!picture_has_no_content(&speckled, stride, width, height));
    }

    /// Indexes: 0 rounds, 1 bytes_in, 2 decode_calls, 3 decoded, 4 delivered,
    /// 5 taken, 6 painted, 7 read_ns, 8 decode_ns, 9 blit_ns, 10 iters,
    /// 11 empty, 12 loop_ns, 13 paint_ns.
    #[test]
    fn sample_reports_stage_measures_per_second() {
        // One second: 50 frames decoded, 1 MB read, 16 ms decoding, 9 ms
        // presenting, 48 frames delivered but only 1 taken.
        let now = counters(&[
            (1, 1_000_000),
            (3, 50),
            (4, 48),
            (5, 1),
            (8, 16_000_000),
            (9, 9_000_000),
        ]);
        let sample = VideoStats::from_deltas(&now, &ZERO, 1.0);
        assert_eq!(sample.frames_decoded, 50);
        assert!((sample.fps - 50.0).abs() < 1e-9);
        assert!((sample.bytes_per_s - 1_000_000.0).abs() < 1e-9);
        assert!((sample.decode_ms - 16.0).abs() < 1e-9);
        assert!((sample.blit_ms - 9.0).abs() < 1e-9);
        // Overwritten counts what the presenter never took (48 - 1), not
        // every delivered frame.
        assert_eq!(sample.overwritten, 47);
    }

    #[test]
    fn sample_uses_the_window_delta_not_the_session_total() {
        let prev = counters(&[(3, 100), (4, 100), (5, 40), (8, 5_000_000)]);
        let now = counters(&[(3, 150), (4, 148), (5, 41), (8, 25_000_000)]);
        let sample = VideoStats::from_deltas(&now, &prev, 2.0);
        assert!((sample.fps - 25.0).abs() < 1e-9); // 50 frames over 2 s
        assert_eq!(sample.overwritten, 47); // 48 delivered - 1 taken
        assert!((sample.decode_ms - 20.0).abs() < 1e-9);
        assert_eq!(sample.frames_decoded, 150); // cumulative, by definition
    }

    #[test]
    fn a_reset_or_coarse_counter_never_underflows() {
        let prev = counters(&[(4, 500), (5, 10), (9, 9_000_000)]);
        let now = counters(&[(4, 3), (5, 9), (9, 1_000)]);
        let sample = VideoStats::from_deltas(&now, &prev, 1.0);
        assert_eq!(sample.overwritten, 0);
        assert_eq!(sample.blit_ms, 0.0);
    }

    #[test]
    fn the_accessor_tracks_what_a_session_publishes() {
        publish(None);
        assert_eq!(stats(), None);
        let sample = VideoStats::from_deltas(&counters(&[(3, 50)]), &ZERO, 1.0);
        publish(Some(sample));
        assert_eq!(stats(), Some(sample));
        publish(None);
        assert_eq!(stats(), None);
    }
}
