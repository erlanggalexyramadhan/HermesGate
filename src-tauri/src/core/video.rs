//! Video: decoder and renderer boundary.
//!
//! Receives the raw H.264 byte stream (scrcpy server in `raw_stream=true`
//! mode) from [`crate::core::mirroring`], decodes it natively through
//! Windows Media Foundation (hardware decode/convert where available) and
//! presents frames in a native Win32 mirror window. Frame data stays in
//! native code and never crosses the JavaScript boundary.
//!
//! The interface is deliberately transport-agnostic: mirroring hands over an
//! already-connected TCP socket (today via `adb forward`, later possibly
//! Wi-Fi) and this module only sees "H.264 bytes in, frames on screen".
//! Decoding and presentation are independent of React, so a future clean
//! floating mirror window can reuse the same pipeline without decoding twice.

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

/// A decoded frame in BGRA order, top-down, `width * 4` bytes per row.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
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
    use std::cell::RefCell;
    use std::io::Read;
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use windows::core::{
        w, implement, BOOL, Error, IUnknown, Ref, Result as WinResult, HSTRING, PCWSTR,
    };
    use windows::Win32::Foundation::{
        E_FAIL, E_INVALIDARG, E_NOTIMPL, FALSE, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, EndPaint, HBRUSH, HDC,
        InvalidateRect, PAINTSTRUCT, SRCCOPY, StretchDIBits,
    };
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Blocking socket reads tick at least this often so Stop and window
    /// close stay responsive while Media Foundation waits for bytes.
    const READ_TICK: Duration = Duration::from_millis(400);

    /// Window class registered once per process.
    const CLASS_NAME: PCWSTR = w!("HermesGateMirror");

    // ---------------------------------------------------------------- socket

    /// `IMFByteStream` backed by the device socket: Media Foundation pulls
    /// raw H.264 bytes straight off the wire (scrcpy `raw_stream` mode has
    /// no framing to parse).
    #[implement(IMFByteStream)]
    struct SocketStream {
        stream: Arc<TcpStream>,
        control: Arc<PlaybackControl>,
        position: AtomicU64,
    }

    impl IMFByteStream_Impl for SocketStream {
        fn GetCapabilities(&self) -> WinResult<u32> {
            // Readable and progressive — never seekable.
            Ok(MFBYTESTREAM_IS_READABLE)
        }

        fn GetLength(&self) -> WinResult<u64> {
            Err(E_NOTIMPL.into())
        }

        fn SetLength(&self, _length: u64) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn GetCurrentPosition(&self) -> WinResult<u64> {
            Ok(self.position.load(Ordering::SeqCst))
        }

        fn SetCurrentPosition(&self, _position: u64) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn IsEndOfStream(&self) -> WinResult<BOOL> {
            Ok(FALSE)
        }

        fn Read(&self, buffer: *mut u8, count: u32, read: *mut u32) -> WinResult<()> {
                    let bytes = unsafe { std::slice::from_raw_parts_mut(buffer, count as usize) };
                    let mut total = 0usize;
                    while total == 0 {
                        if self.control.stop_requested() {
                            break;
                        }
                        match (*self.stream).read(&mut bytes[total..]) {
                            Ok(0) => break,
                            Ok(got) => total += got,
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                                )
                            {
                                // Read tick: loop back and re-check the control flags.
                            }
                            Err(_) => break, // socket lost: Media Foundation sees EOS
                        }
                    }
                    unsafe {
                        *read = total as u32;
                    }
                    if total > 0 {
                        self.position.fetch_add(total as u64, Ordering::SeqCst);
                    }
                    Ok(())
                }

        fn BeginRead(&self, _count: u32) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn EndRead(&self, _cookie: u64) -> WinResult<u32> {
            Err(E_NOTIMPL.into())
        }

        fn Write(&self, _data: *const u8, _count: u32) -> WinResult<u32> {
            Err(E_NOTIMPL.into())
        }

        fn BeginWrite(&self, _count: u32) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn EndWrite(&self, _cookie: u64) -> WinResult<u32> {
            Err(E_NOTIMPL.into())
        }

        fn Seek(
            &self,
            _origin: MFBYTESTREAM_SEEK_ORIGIN,
            _flags: MFBYTESTREAM_SEEK_FLAG,
            _position: *mut i64,
        ) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn Flush(&self) -> WinResult<()> {
            Ok(())
        }

        fn Close(&self) -> WinResult<()> {
            Ok(())
        }
    }

    // -------------------------------------------------------------- decoder

    /// Balances `MFStartup` for one playback session; drops after the reader.
    struct MfGuard;

    impl Drop for MfGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = MFShutdown();
            }
        }
    }

    /// Open the source reader over the socket: raw H.264 in, RGB32 frames
    /// out, with hardware decode/convert where the platform offers it.
    fn open(
        stream: Arc<TcpStream>,
        control: Arc<PlaybackControl>,
    ) -> WinResult<(MfGuard, IMFSourceReader)> {
        unsafe {
            // Dedicated session thread, so COM initialization cannot clash.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
            let guard = MfGuard;

            let mut attributes_ptr: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes_ptr, 2)?;
            let attributes = attributes_ptr.ok_or_else(|| {
                Error::new(E_FAIL, "MFCreateAttributes returned no attribute store")
            })?;
            // Hardware transforms = GPU decode where available; advanced
            // processing = the color conversion the presenter needs.
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;

            let byte_stream: IMFByteStream = SocketStream {
                stream,
                control,
                position: AtomicU64::new(0),
            }
            .into();
            let reader = MFCreateSourceReaderFromByteStream(&byte_stream, &attributes)?;

            let media_type = MFCreateMediaType()?;
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                None,
                &media_type,
            )?;
            Ok((guard, reader))
        }
    }

    /// Current decoded output geometry: `(width, height, stride)`.
    fn output_layout(reader: &IMFSourceReader) -> WinResult<(u32, u32, i32)> {
        unsafe {
            let media_type =
                reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?;
            // MF_MT_FRAME_SIZE packs width into the high 32 bits, height low.
            let packed = media_type.GetUINT64(&MF_MT_FRAME_SIZE)?;
            let width = ((packed >> 32) & 0xFFFF_FFFF) as u32;
            let height = (packed & 0xFFFF_FFFF) as u32;
            let stride = media_type
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .map(|value| value as i32)
                .unwrap_or((width * 4) as i32);
            let stride = if stride == 0 {
                (width * 4) as i32
            } else {
                stride
            };
            Ok((width, height, stride))
        }
    }

    /// Pull one decoded sample out as a normalized top-down BGRA frame.
    fn next_frame(reader: &IMFSourceReader, sample: IMFSample) -> WinResult<Frame> {
        let (width, height, stride) = output_layout(reader)?;
        unsafe {
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data = std::ptr::null_mut();
            let mut max_length = 0u32;
            let mut current_length = 0u32;
            buffer.Lock(&mut data, &mut max_length, &mut current_length)?;
            let frame = normalize(data, current_length, width, height, stride);
            buffer.Unlock()?;
            frame
        }
    }

    /// Copy the locked buffer into a packed top-down frame. Handles negative
    /// stride (bottom-up source) and row padding.
    fn normalize(
        data: *const u8,
        length: u32,
        width: u32,
        height: u32,
        stride: i32,
    ) -> WinResult<Frame> {
        let row_bytes = width as usize * 4;
        let length = length as usize;
        if data.is_null() || width == 0 || height == 0 || length < row_bytes {
            return Err(Error::new(E_INVALIDARG, "decoder produced an empty frame"));
        }
        let mut stride = i64::from(stride);
        if stride == 0 {
            stride = row_bytes as i64;
        }
        let abs_stride = stride.unsigned_abs() as usize;
        if length < abs_stride.saturating_mul(height as usize) {
            if length < row_bytes.saturating_mul(height as usize) {
                return Err(Error::new(
                    E_INVALIDARG,
                    "decoded buffer is smaller than the frame",
                ));
            }
            // Advertised stride disagrees with the buffer: assume packed rows.
            stride = row_bytes as i64;
        }
        let source = unsafe { std::slice::from_raw_parts(data, length) };
        let mut bgra = vec![0u8; row_bytes * height as usize];
        for y in 0..height as usize {
            // Negative stride = bottom-up: the locked buffer starts at the
            // last display row.
            let row = if stride >= 0 { y } else { height as usize - 1 - y };
            let offset = row * abs_stride;
            if offset + row_bytes > source.len() {
                break; // defensive: never read past the locked buffer
            }
            bgra[y * row_bytes..(y + 1) * row_bytes]
                .copy_from_slice(&source[offset..offset + row_bytes]);
        }
        Ok(Frame {
            width,
            height,
            bgra,
        })
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
        ) -> Result<Presenter, String> {
            let title = title.to_string();
            std::thread::Builder::new()
                .name("mirror-window".to_string())
                .spawn(move || window_loop(slot, control, &title, size))
                .map(|thread| Presenter { thread })
                .map_err(|error| format!("cannot start mirror window: {error}"))
        }

        fn join(self) {
            let _ = self.thread.join();
        }
    }

    /// Per-window thread state, accessed from the window procedure.
    struct WindowState {
        control: Option<Arc<PlaybackControl>>,
        latest: Option<Frame>,
    }

    thread_local! {
        static STATE: RefCell<Option<WindowState>> = const { RefCell::new(None) };
    }

    fn window_loop(
        slot: Arc<Mutex<Option<Frame>>>,
        control: Arc<PlaybackControl>,
        title: &str,
        initial_size: (u32, u32),
    ) {
        STATE.with(|state| {
            *state.borrow_mut() = Some(WindowState {
                control: Some(control.clone()),
                latest: None,
            });
        });

        let hinstance = match unsafe { GetModuleHandleW(None) } {
            Ok(handle) => handle,
            Err(_) => {
                control.window_dead.store(true, Ordering::SeqCst);
                return;
            }
        };
        register_class(hinstance);
        let hwnd = match create_window(hinstance, title, initial_size) {
            Some(hwnd) => hwnd,
            None => {
                control.window_dead.store(true, Ordering::SeqCst);
                return;
            }
        };

        let mut size = initial_size;
        'outer: loop {
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
            if !unsafe { IsWindow(hwnd) }.as_bool() {
                break;
            }
            let fresh = slot.lock().map(|mut frame| frame.take()).unwrap_or(None);
            if let Some(frame) = fresh {
                if (frame.width, frame.height) != size {
                    size = (frame.width, frame.height);
                    resize_client(hwnd, size);
                }
                STATE.with(|state| {
                    if let Some(window) = state.borrow_mut().as_mut() {
                        window.latest = Some(frame);
                    }
                });
                let _ = unsafe { InvalidateRect(hwnd, None, false) };
            } else {
                std::thread::sleep(Duration::from_millis(6));
            }
        }

        STATE.with(|state| *state.borrow_mut() = None);
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
                hIconSm: HICON(std::ptr::null()),
            };
            unsafe {
                RegisterClassExW(&class);
            }
        });
    }

    fn create_window(hinstance: HINSTANCE, title: &str, size: (u32, u32)) -> Option<HWND> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: size.0 as i32,
            bottom: size.1 as i32,
        };
        let _ = unsafe {
            AdjustWindowRectEx(&mut rect, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0))
        };
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT as i32,
                CW_USEDEFAULT as i32,
                rect.right - rect.left,
                rect.bottom - rect.top,
                None,
                None,
                Some(hinstance),
                None,
            )
        };
        created.ok()
    }

    fn resize_client(hwnd: HWND, size: (u32, u32)) {
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
            WM_CLOSE => {
                // User-initiated close: classify the session end as clean.
                STATE.with(|state| {
                    if let Some(window) = state.borrow().as_ref().and_then(|w| w.control.as_ref())
                    {
                        window.user_closed.store(true, Ordering::SeqCst);
                    }
                });
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                STATE.with(|state| {
                    if let Some(window) = state.borrow().as_ref().and_then(|w| w.control.as_ref())
                    {
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
        let mut paint_struct = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &mut paint_struct) };
        let mut client = RECT::default();
        let _ = unsafe { GetClientRect(hwnd, &mut client) };
        STATE.with(|state| {
            if let Some(window) = state.borrow().as_ref() {
                if let Some(frame) = window.latest.as_ref() {
                    blit(hdc, frame, &client);
                }
            }
        });
        let _ = unsafe { EndPaint(hwnd, &paint_struct) };
    }

    /// Draw the frame stretched into the client area (top-down BGRA → DIB).
    fn blit(hdc: HDC, frame: &Frame, client: &RECT) {
        let dest_w = client.right - client.left;
        let dest_h = client.bottom - client.top;
        if dest_w <= 0 || dest_h <= 0 {
            return;
        }
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: frame.width as i32,
            biHeight: -(frame.height as i32), // negative = top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
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
                frame.width as i32,
                frame.height as i32,
                Some(frame.bgra.as_ptr().cast()),
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
        // The byte stream must tick instead of blocking forever, otherwise
        // Stop would wait for the next video packet.
        let _ = stream.set_read_timeout(Some(READ_TICK));
        let _ = stream.set_nodelay(true);

        let (guard, reader) = match open(stream, control.clone()) {
            Ok(opened) => opened,
            Err(error) => {
                return StreamEnd::Failed(format!("video decoder init failed: {error}"));
            }
        };

        let slot: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
        let mut presenter: Option<Presenter> = None;
        let mut size: Option<(u32, u32)> = None;
        let mut end = StreamEnd::Disconnected;
        loop {
            if control.stop_requested() {
                end = control_end(&control);
                break;
            }
            let mut flags = 0u32;
            let mut timestamp = 0i64;
            let mut sample: Option<IMFSample> = None;
            let read = unsafe {
                reader.ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut timestamp),
                    Some(&mut sample),
                )
            };
            if let Err(error) = read {
                end = or_failure(&control, format!("video decoder failed: {error}"));
                break;
            }
            if flags & (MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                end = control_end(&control);
                break;
            }
            if flags & (MF_SOURCE_READERF_ERROR.0 as u32) != 0 {
                end = or_failure(&control, "video decoder reported an error".to_string());
                break;
            }
            let Some(sample) = sample else {
                continue;
            };
            let frame = match next_frame(&reader, sample) {
                Ok(frame) => frame,
                Err(error) => {
                    end = or_failure(&control, format!("frame conversion failed: {error}"));
                    break;
                }
            };
            let frame_size = (frame.width, frame.height);
            if size != Some(frame_size) {
                size = Some(frame_size);
                on_size(frame.width, frame.height);
                if presenter.is_none() {
                    match Presenter::spawn(slot.clone(), control.clone(), title, frame_size) {
                        Ok(spawned) => presenter = Some(spawned),
                        Err(error) => {
                            end = StreamEnd::Failed(error);
                            break;
                        }
                    }
                }
            }
            if let Ok(mut latest) = slot.lock() {
                *latest = Some(frame);
            }
        }

        // Teardown order: window first, then reader, then MFShutdown.
        control.stop.store(true, Ordering::SeqCst);
        if let Some(presenter) = presenter {
            presenter.join();
        }
        drop(reader);
        drop(guard);
        end
    }
}
