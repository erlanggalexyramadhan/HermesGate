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
    use std::cell::RefCell;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, EndPaint, InvalidateRect, StretchDIBits, UpdateWindow, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBRUSH, HDC, PAINTSTRUCT, SRCCOPY,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Blocking socket reads tick at least this often so Stop and window
    /// close stay responsive while Media Foundation waits for bytes.
    const READ_TICK: Duration = Duration::from_millis(400);

    /// Window class registered once per process.
    const CLASS_NAME: PCWSTR = w!("HermesGateMirror");

    // ----------------------------------------------------------- measurement

    /// TEMPORARY diagnostics: counters for the existing pipeline stages.
    /// Enabled only when `HERMESGATE_VIDEO_STATS` names a CSV file; the
    /// session thread is the only writer of that file.
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
        /// `Some` when the CSV path is configured and creatable.
        fn open() -> Option<(Arc<Stats>, std::fs::File)> {
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
            Some((Arc::new(Stats::default()), file))
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
        // Decoding on the device is opt-in until the deadlock below is fixed:
        // the Media Foundation H.264 decoder accepts the Direct3D 11 device
        // manager and then never returns from its first `ProcessOutput` (see
        // `tests/h264_offline.rs`), so the software decoder stays the default.
        // `HERMESGATE_H264_HW=1` asks for the device path anyway.
        if std::env::var("HERMESGATE_H264_HW").is_err() {
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
            device: Option<Arc<gpu::Device>>,
            stats: Option<Arc<Stats>>,
        ) -> Result<Presenter, String> {
            let title = title.to_string();
            std::thread::Builder::new()
                .name("mirror-window".to_string())
                .spawn(move || window_loop(slot, control, &title, size, device, stats))
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
        stats: &Option<Arc<Stats>>,
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
                    device.clone(),
                    stats.clone(),
                )?);
            }
        }
        if let Ok(mut latest) = slot.lock() {
            *latest = Some(frame);
        }
        if let Some(stats) = stats {
            Stats::add(&stats.delivered, 1);
        }
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
        /// TEMPORARY diagnostics (see [`Stats`]).
        stats: Option<Arc<Stats>>,
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
        fn new(device: Option<Arc<gpu::Device>>, hwnd: HWND, geometry: (u32, u32)) -> Self {
            let presenter = device.as_ref().and_then(|device| {
                match gpu::Presenter::new(device.clone(), hwnd, client_size(hwnd), geometry) {
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
        device: Option<Arc<gpu::Device>>,
        stats: Option<Arc<Stats>>,
    ) {
        STATE.with(|state| {
            *state.borrow_mut() = Some(WindowState {
                control: Some(control.clone()),
                latest: None,
                resize: None,
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
        let mut presentation = Presentation::new(device.clone(), hwnd, initial_size);
        let mut size = initial_size;
        'outer: loop {
            let iteration = Instant::now();
            if let Some(stats) = &stats {
                Stats::add(&stats.iters, 1);
            }
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
                if let Some(stats) = &stats {
                    Stats::add(&stats.taken, 1);
                }
                // A rotated stream gets a new window size and a fresh swap
                // chain: surfaces and video processor are sized for the
                // picture they were built for.
                let frame_size = frame.size();
                if frame_size != size {
                    size = frame_size;
                    resize_client(hwnd, size);
                    presentation = Presentation::new(device.clone(), hwnd, size);
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
                if let Some(stats) = &stats {
                    Stats::add(&stats.blit_ns, started.elapsed().as_nanos() as u64);
                }
            } else {
                if let Some(stats) = &stats {
                    Stats::add(&stats.empty, 1);
                }
                std::thread::sleep(Duration::from_millis(6));
            }
            if let Some(stats) = &stats {
                Stats::add(&stats.loop_ns, iteration.elapsed().as_nanos() as u64);
            }
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
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: size.0 as i32,
            bottom: size.1 as i32,
        };
        let _ = unsafe {
            AdjustWindowRectEx(&mut rect, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0))
        };
        let window_w = rect.right - rect.left;
        let window_h = rect.bottom - rect.top;
        // Centre it: a default position can put a tall window's lower half
        // off the bottom of the desktop.
        let (screen_w, screen_h) = screen();
        let x = ((screen_w - window_w) / 2).max(0);
        let y = ((screen_h - window_h) / 2).max(0);
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW,
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
                    if let Some(stats) = &window.stats {
                        Stats::add(&stats.painted, 1);
                        Stats::add(&stats.blit_ns, started.elapsed().as_nanos() as u64);
                    }
                }
            }
        });
        let _ = unsafe { EndPaint(hwnd, &paint_struct) };
        STATE.with(|state| {
            if let Some(window) = state.borrow().as_ref() {
                if let Some(stats) = &window.stats {
                    Stats::add(&stats.paint_ns, entered.elapsed().as_nanos() as u64);
                }
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
        // The Direct3D 11 path is opt-in: it feeds the decoder from the device
        // and presents through a swap chain, but device decoding deadlocks
        // inside Media Foundation on this stack (see `docs/adr/0003`), so the
        // default pipeline stays on the verified software decoder plus the GDI
        // presenter. `HERMESGATE_VIDEO_GPU=1` builds the device path.
        let device = if std::env::var("HERMESGATE_VIDEO_GPU").is_err() {
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

        // TEMPORARY measurement state (see [`Stats`]).
        let (stats, mut csv) = match Stats::open() {
            Some((stats, file)) => (Some(stats), Some(file)),
            None => (None, None),
        };
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
            if let Some(stats) = &stats {
                Stats::add(&stats.read_ns, read_start.elapsed().as_nanos() as u64);
                Stats::add(&stats.rounds, 1);
            }
            let (filled, last) = match outcome {
                ReadOutcome::Data(filled) => {
                    if let Some(stats) = &stats {
                        Stats::add(&stats.bytes_in, filled as u64);
                    }
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
            if let Some(stats) = &stats {
                Stats::add(&stats.decode_ns, decode_start.elapsed().as_nanos() as u64);
                Stats::add(&stats.decode_calls, 1);
            }
            let frames = match decoded {
                Ok(frames) => frames,
                Err(error) => {
                    end = or_failure(&control, format!("video decode failed: {error}"));
                    break;
                }
            };

            if !logged_mode {
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
                if let Some(stats) = &stats {
                    Stats::add(&stats.decoded, 1);
                }
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

            // TEMPORARY: one CSV row per second, deltas over that second.
            if let (Some(stats), Some(file)) = (&stats, csv.as_mut()) {
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
                    let _ = writeln!(
                        file,
                        "{:.1},{},{},{:.0},{},{},{:.1},{},{},{},{},{:.1},{:.1},{:.1},{:.0},{:.0},\
                         {},{},{:.1},{:.1}",
                        started.elapsed().as_secs_f64(),
                        now[0],
                        now[1],
                        delta(1) as f64 / secs,
                        now[2],
                        now[3],
                        delta(3) as f64 / secs,
                        now[4],
                        now[5],
                        now[6],
                        delta(4).saturating_sub(delta(5)),
                        delta(7) as f64 / 1e6,
                        delta(8) as f64 / 1e6,
                        delta(9) as f64 / 1e6,
                        delta(7) as f64 / (secs * 1e9) * 100.0,
                        delta(8) as f64 / (secs * 1e9) * 100.0,
                        delta(10),
                        delta(11),
                        delta(12) as f64 / 1e6,
                        delta(13) as f64 / 1e6
                    );
                    let _ = file.flush();
                    prev = now;
                    window = Instant::now();
                }
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
        end
    }
}
