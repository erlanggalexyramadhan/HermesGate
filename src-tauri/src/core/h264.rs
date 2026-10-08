//! H.264 decoder: the Windows Media Foundation decoder MFT.
//!
//! A containerless Annex-B H.264 stream (scrcpy in `raw_stream=true` mode)
//! cannot be opened by a Media Foundation *source reader* — the source
//! resolver has no byte-stream handler for a raw elementary stream and
//! answers `0xC00D36C4` (`MF_E_UNSUPPORTED_BYTESTREAM_TYPE`) — while the
//! H.264 *decoder MFT* does consume it. So the MFT is driven here directly:
//! Annex-B bytes in, split into access units, decoded pictures out.
//!
//! Two decode modes share one state machine:
//!
//! * **Device (hardware)**: [`Decoder::new_hardware`] hands the MFT a
//!   Direct3D 11 device through its DXGI device manager, and the decoder
//!   reports back (`MF_SA_D3D11_AWARE`) whether it accepted it. Accepted
//!   pictures stay in device memory as NV12 textures; each one is copied into
//!   a ring surface the decode thread owns — Media Foundation recycles its own
//!   textures — and handed out as [`Frame::Texture`] for the GPU presenter in
//!   [`crate::core::gpu`]. No pixel crosses the CPU.
//! * **System memory (software)**: [`Decoder::new`] offers no device, and each
//!   decoded picture is converted to BGRA here ([`Frame::Bgra`]).
//!
//! Which mode runs is decided by the platform, not by the caller: a decoder
//! that does not confirm the device, or a device path that fails at runtime,
//! drops back to system memory instead of failing the session.

use std::fmt;

/// Failure while creating, feeding or draining the decoder.
///
/// The message is meant for the UI banner, so it names the stage that failed
/// rather than exposing a bare `HRESULT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H264Error {
    message: String,
}

impl H264Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The user-facing message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for H264Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for H264Error {}

/// The geometry of the decoder's pictures.
///
/// The decoder works on a coded size (720x1616 rows for a 720x1604 phone
/// screen) and shows a visible picture inside it (the display aperture,
/// 720x1604). Both matter: the coded size is how much picture there is, the
/// visible size is what belongs on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    /// The size the decoder writes, padding included.
    pub coded: (u32, u32),
    /// The picture inside the coded size.
    pub visible: (u32, u32),
}

/// One decoded frame.
pub enum Frame {
    /// A picture in device memory, on its way to the GPU presenter.
    #[cfg(windows)]
    Texture {
        /// The texture holding the picture (one of the decode-side ring
        /// surfaces).
        texture: crate::core::gpu::Texture,
        /// Coded size of the picture, padding included.
        coded: (u32, u32),
        /// The visible picture inside the coded size.
        visible: (u32, u32),
    },
    /// A picture in system memory, top-down BGRA,
    /// `bgra.len() == stride * height`.
    Bgra {
        width: u32,
        height: u32,
        /// Bytes per row, taken from the decoder's own buffer (may exceed
        /// `width * 4`).
        stride: u32,
        bgra: Vec<u8>,
    },
}

impl Frame {
    /// The picture size to show.
    pub fn size(&self) -> (u32, u32) {
        match self {
            #[cfg(windows)]
            Frame::Texture { visible, .. } => *visible,
            Frame::Bgra { width, height, .. } => (*width, *height),
        }
    }
}

/// The public decoder boundary: [`Decoder::new`], [`Decoder::push`],
/// [`Decoder::finish`].
#[cfg(windows)]
pub struct Decoder {
    inner: native::DecoderState,
}

#[cfg(windows)]
impl Decoder {
    /// Create and initialise the Media Foundation H.264 decoder MFT, decoding
    /// into system memory.
    ///
    /// Takes no arguments: the stream geometry is not known before the first
    /// SPS arrives, so the MFT learns it from the bitstream and reports it
    /// back on the output media type.
    pub fn new() -> Result<Self, H264Error> {
        Ok(Self {
            inner: native::DecoderState::new()?,
        })
    }

    /// Create a decoder that decodes into `device` textures.
    ///
    /// The MFT is bound to the device through a DXGI device manager and asked
    /// for its Direct3D 11 output type, so pictures stay in device memory
    /// ([`Frame::Texture`]) and the GPU converts and presents them without a
    /// pixel crossing the CPU.
    ///
    /// This is a request, not a guarantee: a decoder that does not confirm the
    /// device (`MF_SA_D3D11_AWARE`) keeps decoding in system memory
    /// ([`Frame::Bgra`]), which is also where a device path that fails later
    /// lands.
    pub fn new_hardware(
        device: std::sync::Arc<crate::core::gpu::Device>,
    ) -> Result<Self, H264Error> {
        Ok(Self {
            inner: native::DecoderState::new_hardware(device)?,
        })
    }

    /// The decoder's picture geometry, once it has seen the stream's SPS.
    pub fn geometry(&self) -> Option<Geometry> {
        self.inner.geometry()
    }

    /// True while decoded pictures are handed out as device textures.
    pub fn is_hardware(&self) -> bool {
        self.inner.is_hardware()
    }

    /// Feed raw Annex-B H.264 bytes and return whatever frames became
    /// decodable.
    ///
    /// A `chunk` may hold zero, one or several complete access units and may
    /// end mid-NAL: the partial tail is buffered and completed by a later
    /// call. An empty return is normal while the decoder waits for the next
    /// access unit.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, H264Error> {
        self.inner.push(chunk)
    }

    /// Drain the decoder after the last call to [`Decoder::push`].
    ///
    /// Signals end of stream to the MFT and collects the frames it still
    /// holds. Call it once; further `push` calls are still accepted but the
    /// stream is considered finished.
    pub fn finish(&mut self) -> Result<Vec<Frame>, H264Error> {
        self.inner.finish()
    }
}

#[cfg(not(windows))]
pub struct Decoder;

#[cfg(not(windows))]
impl Decoder {
    /// Media Foundation does not exist outside Windows, so decoding is
    /// unavailable.
    pub fn new() -> Result<Self, H264Error> {
        Err(H264Error::new(
            "H.264 decoding is only supported on Windows",
        ))
    }

    pub fn push(&mut self, _chunk: &[u8]) -> Result<Vec<Frame>, H264Error> {
        Err(H264Error::new(
            "H.264 decoding is only supported on Windows",
        ))
    }

    pub fn finish(&mut self) -> Result<Vec<Frame>, H264Error> {
        Err(H264Error::new(
            "H.264 decoding is only supported on Windows",
        ))
    }

    /// No decoder, so no geometry.
    pub fn geometry(&self) -> Option<Geometry> {
        None
    }

    /// Device decoding does not exist outside Windows.
    pub fn is_hardware(&self) -> bool {
        false
    }
}

#[cfg(windows)]
mod native {
    /// How many output media types the decoder is asked for before it is assumed
    /// to have offered them all.
    const MAX_OFFERED_OUTPUT_TYPES: u32 = 32;

    /// How many decoded frames a device decoder keeps in flight: enough for
    /// the compositor to hold one while the next is being decoded, and it is
    /// what the DXVA frame manager sizes its texture pool from.
    const STREAMING_SAMPLE_COUNT: u32 = 6;

    use super::{Frame, Geometry, H264Error};
    use crate::core::gpu::{self, Copier};
    use crate::core::pixels;

    use std::mem::ManuallyDrop;
    use std::sync::Arc;

    use windows::core::PWSTR;
    use windows::Win32::Graphics::Direct3D11::D3D11_BIND_DECODER;
    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, IMFMediaBuffer, IMFMediaType, IMFSample, IMFTransform, MFCreateMediaType,
        MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFShutdown, MFStartup, MFTEnumEx,
        MFT_FRIENDLY_NAME_Attribute, MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoFormat_RGB32,
        MFSTARTUP_FULL, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_HARDWARE,
        MFT_ENUM_FLAG_SORTANDFILTER, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
        MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER,
        MFT_OUTPUT_DATA_BUFFER_INCOMPLETE, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
        MFT_REGISTER_TYPE_INFO, MF_E_TRANSFORM_ASYNC_LOCKED, MF_E_TRANSFORM_NEED_MORE_INPUT,
        MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE,
        MF_MT_GEOMETRIC_APERTURE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE,
        MF_SA_D3D11_AWARE, MF_SA_D3D11_BINDFLAGS, MF_SA_MINIMUM_OUTPUT_SAMPLE_COUNT,
        MF_SA_MINIMUM_OUTPUT_SAMPLE_COUNT_PROGRESSIVE, MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER,
    };

    /// Bytes of one 30 fps frame in 100 ns units.
    const FRAME_DURATION: i64 = 333_333;

    /// Consecutive `push` calls that produced no output before the decoder is
    /// considered wedged.
    const MAX_IDLE_PULLS: u32 = 64;

    /// Device-mode failures tolerated before the session decodes in system
    /// memory for good. One retry covers a device path that needs a second
    /// attempt to settle (a stream change, a driver hiccup).
    const HARDWARE_RETRIES: u32 = 1;

    /// An MPEG-4 / H.264 Annex-B start code: 4-byte form is checked first.
    const START_CODE_3: [u8; 3] = [0, 0, 1];
    const START_CODE_4: [u8; 4] = [0, 0, 0, 1];

    /// Balances one [`MFStartup`] for the lifetime of a decoder.
    struct MfGuard;

    impl Drop for MfGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = MFShutdown();
            }
        }
    }

    /// The MFT plus the byte accumulation state that turns a chunked
    /// Annex-B byte stream into access units.
    pub(super) struct DecoderState {
        transform: IMFTransform,
        _mf: MfGuard,
        input_set: bool,
        output_set: bool,
        dimensions: Option<(u32, u32)>,
        /// Coded and visible picture size, once negotiated.
        geometry: Option<Geometry>,
        /// The device the decoder writes pictures into, when device decoding
        /// was requested.
        device: Option<Arc<gpu::Device>>,
        /// The manager the MFT was bound to: it has to outlive the binding.
        manager: Option<gpu::DeviceManager>,
        /// The copy ring decoded textures are read through.
        copier: Option<Copier>,
        /// Decode calls seen so far.
        pushes: u64,
        /// True once the device was offered to the MFT, whatever it answered.
        device_bound: bool,
        /// True when the MFT confirmed it decodes into device textures.
        device_accepted: bool,
        /// True while pictures are handed out as device textures.
        hardware: bool,
        /// Device-mode retries left before system-memory decoding for good.
        hardware_retries: u32,
        /// Pushes to wait before asking for device textures again.
        cooldown: u32,
        /// Annex-B bytes not yet split into a complete access unit.
        pending: Vec<u8>,
        /// Access units not yet accepted by `ProcessInput`.
        ready: Vec<Vec<u8>>,
        /// Count of input samples already handed to the MFT.
        sample_index: i64,
        /// `push` calls with no output, reset whenever a frame comes out.
        idle_pulls: u32,
        draining: bool,
    }

    impl DecoderState {
        pub(super) fn new() -> Result<Self, H264Error> {
            Self::create(None)
        }

        /// Create a decoder that decodes into `device` textures when the
        /// platform supports it.
        pub(super) fn new_hardware(device: Arc<gpu::Device>) -> Result<Self, H264Error> {
            Self::create(Some(device))
        }

        fn create(device: Option<Arc<gpu::Device>>) -> Result<Self, H264Error> {
            unsafe {
                // Multithreaded apartment; the session thread already owns it
                // in some flows, which is not an error here.
                let _ = CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
                MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| Self::error("MFStartup", e))?;
                let guard = MfGuard;
                // A device decoder wants a vendor hardware MFT: the in-box
                // Microsoft one only *reports* Direct3D 11 awareness and then
                // never answers `ProcessOutput`. When the machine has no such
                // MFT, the in-box decoder keeps the session alive instead.
                let transform: IMFTransform = if device.is_some() {
                    match Self::activate_hardware_mft() {
                        Some(transform) => transform,
                        None => {
                            Self::debug_log(
                                "no Direct3D 11 aware hardware H.264 MFT, using the in-box decoder",
                            );
                            Self::in_box_mft()?
                        }
                    }
                } else {
                    Self::in_box_mft()?
                };

                // Best effort: a live source wants the low-latency profile,
                // but not every MFT exposes a writable attribute store.
                if let Ok(attributes) = transform.GetAttributes() {
                    let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
                }

                Ok(Self {
                    transform,
                    _mf: guard,
                    input_set: false,
                    output_set: false,
                    dimensions: None,
                    geometry: None,
                    device,
                    manager: None,
                    copier: None,
                    pushes: 0,
                    device_bound: false,
                    device_accepted: false,
                    hardware: false,
                    hardware_retries: HARDWARE_RETRIES,
                    cooldown: 0,
                    pending: Vec::new(),
                    ready: Vec::new(),
                    sample_index: 0,
                    idle_pulls: 0,
                    draining: false,
                })
            }
        }

        fn error(stage: &str, error: windows::core::Error) -> H264Error {
            H264Error::new(format!("{stage}: {error}"))
        }

        /// The in-box Microsoft H.264 decoder MFT: what every session used
        /// before hardware MFTs were enumerated.
        fn in_box_mft() -> Result<IMFTransform, H264Error> {
            unsafe {
                CoCreateInstance(
                    &windows::Win32::Media::MediaFoundation::CLSID_MSH264DecoderMFT,
                    None::<&windows::core::IUnknown>,
                    CLSCTX_INPROC_SERVER,
                )
                .map_err(|e| Self::error("creating the H.264 decoder MFT", e))
            }
        }

        /// Enumerate the vendor hardware H.264 decoders and activate the
        /// first one that decodes into Direct3D 11 textures.
        ///
        /// A hardware MFT is asynchronous: everything but its attribute store
        /// answers `MF_E_TRANSFORM_ASYNC_LOCKED`, so `MF_TRANSFORM_ASYNC_UNLOCK`
        /// is written right after activation, before anything else is asked
        /// of it. `None` means the machine has no Direct3D 11 aware hardware
        /// H.264 decoder, and the caller stays on the in-box MFT.
        fn activate_hardware_mft() -> Option<IMFTransform> {
            unsafe {
                // Registered input type filter first: the video decoder
                // category also holds the MJPEG, MPEG-4 Visual, HEVC and AV1
                // hardware decoders, and an H.264 stream must not land on one.
                // The output type stays unfiltered, so a decoder that only
                // offers it after the device is bound still shows up.
                let input = MFT_REGISTER_TYPE_INFO {
                    guidMajorType: MFMediaType_Video,
                    guidSubtype: MFVideoFormat_H264,
                };
                for registered_filter in [true, false] {
                    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
                    let mut count = 0u32;
                    if let Err(error) = MFTEnumEx(
                        MFT_CATEGORY_VIDEO_DECODER,
                        MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                        if registered_filter {
                            Some(&input)
                        } else {
                            None
                        },
                        None,
                        &mut activates,
                        &mut count,
                    ) {
                        Self::debug_log(format!(
                            "enumerating hardware decoder MFTs failed: {error}"
                        ));
                        continue;
                    }

                    // Media Foundation allocated the array: the references are
                    // moved out of it, then the array itself is freed, so
                    // neither leaks whichever candidate is picked.
                    let mut candidates = Vec::with_capacity(count as usize);
                    if !activates.is_null() {
                        for index in 0..count as usize {
                            candidates.push(activates.add(index).read());
                        }
                        CoTaskMemFree(Some(activates as *const _));
                    }

                    for activate in candidates.into_iter().flatten() {
                        let name = Self::mft_name(&activate);
                        let declared_aware =
                            activate.GetUINT32(&MF_SA_D3D11_AWARE).unwrap_or(0) != 0;
                        let Ok(transform) = activate.ActivateObject::<IMFTransform>() else {
                            Self::debug_log(format!("skipping {name}: it will not activate"));
                            continue;
                        };
                        let Ok(attributes) = transform.GetAttributes() else {
                            Self::debug_log(format!("skipping {name}: it has no attributes"));
                            continue;
                        };
                        // First thing on an async MFT, before any other call
                        // may be answered with MF_E_TRANSFORM_ASYNC_LOCKED.
                        let _ = attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1);
                        if !declared_aware
                            && attributes.GetUINT32(&MF_SA_D3D11_AWARE).unwrap_or(0) == 0
                        {
                            Self::debug_log(format!("skipping {name}: not Direct3D 11 aware"));
                            continue;
                        }
                        // The filtered pass proved H.264 against the registry;
                        // the unfiltered pass has to prove it against the MFT,
                        // because some drivers register their codec list
                        // incompletely.
                        if !registered_filter && !Self::accepts_h264(&transform) {
                            Self::debug_log(format!("skipping {name}: it does not decode H.264"));
                            continue;
                        }
                        Self::debug_log(format!("using the hardware decoder MFT: {name}"));
                        return Some(transform);
                    }
                }
                None
            }
        }

        /// Whether the decoder declares H.264 among the types it accepts on
        /// its first input stream.
        fn accepts_h264(transform: &IMFTransform) -> bool {
            let mut index = 0u32;
            while let Ok(media_type) = unsafe { transform.GetInputAvailableType(0, index) } {
                let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) };
                if subtype
                    .map(|subtype| subtype == MFVideoFormat_H264)
                    .unwrap_or(false)
                {
                    return true;
                }
                index += 1;
                if index >= 32 {
                    break;
                }
            }
            false
        }

        /// The registered name of an enumerated MFT, for the debug log.
        fn mft_name(activate: &IMFActivate) -> String {
            let unnamed = String::from("an unnamed hardware MFT");
            let mut text = PWSTR::null();
            let mut length = 0u32;
            let named = unsafe {
                activate
                    .GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut text, &mut length)
                    .is_ok()
            };
            if !named {
                return unnamed;
            }
            let name = unsafe { text.to_string() }.unwrap_or_default();
            unsafe { CoTaskMemFree(Some(text.as_ptr() as *const _)) };
            if name.is_empty() {
                unnamed
            } else {
                name
            }
        }

        /// One line of decoder diagnostics, printed only when
        /// `HERMESGATE_H264_DEBUG` is set.
        fn debug_log(message: impl std::fmt::Display) {
            if std::env::var_os("HERMESGATE_H264_DEBUG").is_some() {
                eprintln!("mirror: {message}");
            }
        }

        pub(super) fn geometry(&self) -> Option<Geometry> {
            self.geometry
        }

        pub(super) fn is_hardware(&self) -> bool {
            self.hardware
        }

        /// True when decoded pictures are wanted as device textures.
        fn hardware_requested(&self) -> bool {
            self.cooldown == 0
                && self.device_accepted
                && self
                    .device
                    .as_ref()
                    .is_some_and(|device| device.is_hardware())
        }

        /// Offer the MFT the device it should decode into, once, and record
        /// whether it took it.
        ///
        /// A decoder that refuses the device, or a machine whose driver does
        /// not provide the DXGI device manager, is not an error: the MFT keeps
        /// decoding into system memory.
        fn bind_device(&mut self) {
            if self.device_bound {
                return;
            }
            self.device_bound = true;
            let Some(device) = self.device.clone() else {
                return;
            };
            let manager = match gpu::device_manager(&device) {
                Ok(manager) => manager,
                Err(error) => {
                    eprintln!("mirror: decoding on the CPU, no GPU device manager: {error}");
                    return;
                }
            };
            if let Err(error) = unsafe {
                self.transform.ProcessMessage(
                    MFT_MESSAGE_SET_D3D_MANAGER,
                    windows::core::Interface::as_raw(manager.raw()) as usize,
                )
            } {
                // An async hardware MFT that stayed locked means the unlock
                // attribute did not take: either way the session decodes in
                // system memory from here on.
                if error.code() == MF_E_TRANSFORM_ASYNC_LOCKED {
                    eprintln!(
                        "mirror: the decoder is still locked to its async pipeline, decoding on the CPU: {error}"
                    );
                } else {
                    eprintln!("mirror: the decoder refused the GPU device: {error}");
                }
                return;
            }
            // The MFT reports whether it actually decodes into textures.
            self.device_accepted = unsafe { self.transform.GetAttributes() }
                .ok()
                .and_then(|attributes| unsafe { attributes.GetUINT32(&MF_SA_D3D11_AWARE).ok() })
                .unwrap_or(0)
                != 0;
            if !self.device_accepted {
                eprintln!("mirror: the decoder is not Direct3D 11 aware, decoding on the CPU");
                return;
            }
            self.manager = Some(manager);
            Self::debug_log("the decoder accepted the Direct3D 11 device manager");
        }

        /// Drop the device path for the rest of the session.
        fn degrade_to_software(&mut self, why: String) {
            eprintln!("mirror: decoding on the CPU ({why})");
            self.device = None;
            self.manager = None;
            self.copier = None;
            self.device_bound = true;
            self.device_accepted = false;
            self.hardware = false;
            // Renegotiate: the output type has to stop being a device type.
            self.output_set = false;
        }

        /// Handle a device-path failure: retry once, then decode in system
        /// memory. Never fails the session.
        fn retry_or_degrade(&mut self, why: String) {
            if self.hardware_retries > 0 && self.pushes > 4 {
                self.hardware_retries -= 1;
                eprintln!("mirror: retrying GPU decoding ({why})");
                self.hardware = false;
                self.copier = None;
                self.output_set = false;
                self.cooldown = 1;
            } else {
                self.degrade_to_software(why);
            }
        }

        pub(super) fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, H264Error> {
            self.cooldown = self.cooldown.saturating_sub(1);
            self.pushes += 1;
            self.pending.extend_from_slice(chunk);
            self.split_access_units();
            self.drive()
        }

        pub(super) fn finish(&mut self) -> Result<Vec<Frame>, H264Error> {
            if !self.draining {
                if !self.pending.is_empty() {
                    self.ready.push(std::mem::take(&mut self.pending));
                }
                if self.input_set && self.output_set {
                    unsafe {
                        self.transform
                            .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                            .map_err(|e| Self::error("draining the decoder", e))?;
                    }
                }
                self.draining = true;
            }

            let mut frames = Vec::new();
            loop {
                self.ensure_media_types()?;
                let progressed = self.flush_ready_input()?;
                let before = self.idle_pulls;
                frames.extend(self.pull_output()?);
                // A flush that submitted an access unit is progress even when
                // it produced no frame yet: the newly submitted input may be
                // what unlocks output negotiation, so keep looping for it.
                if self.idle_pulls == before && !progressed {
                    break;
                }
                if self.idle_pulls == before {
                    continue;
                }
                self.idle_pulls += 1;
                if self.idle_pulls >= MAX_IDLE_PULLS {
                    break;
                }
            }
            Ok(frames)
        }

        /// Split `self.pending` into complete access units, keeping the
        /// trailing partial NAL buffered.
        ///
        /// The bitstream is Annex-B: NAL units are introduced by a 3- or
        /// 4-byte start code. Parameter sets (SPS 7, PPS 8) and SEI (6) are
        /// kept with the picture they precede: an access unit ends at the
        /// last VCL NAL (type 1 or 5) of the picture.
        fn split_access_units(&mut self) {
            let mut units: Vec<(usize, usize, u8)> = Vec::new();
            let mut cursor = 0usize;

            while let Some((position, code_len)) = find_start_code(&self.pending, cursor) {
                let nal_type = self
                    .pending
                    .get(position + code_len)
                    .map_or(0, |b| b & 0x1f);
                units.push((position, code_len, nal_type));
                cursor = position + code_len;
            }

            if units.is_empty() {
                return;
            }

            // The picture ends at the last VCL NAL, whatever its type: a
            // stream whose trailing NALs are P-frames must still yield access
            // units, so type 5 is not preferred over type 1 here.
            let picture_no = units.iter().rposition(|(_, _, nal_type)| is_vcl(*nal_type));

            let boundary = match picture_no {
                Some(picture_no) => {
                    // The non-VCL NALs directly before a VCL NAL belong to the
                    // same access unit, so walk back over them.
                    let mut head = picture_no;
                    while head > 0 && !is_vcl(units[head - 1].2) {
                        head -= 1;
                    }
                    units[head].0
                }
                // Only non-VCL NALs so far: keep the last one, the earlier
                // ones can never complete a picture.
                None => units[units.len() - 1].0,
            };

            if boundary == 0 {
                return;
            }

            let rest = self.pending.split_off(boundary);
            self.ready.push(std::mem::replace(&mut self.pending, rest));
        }

        fn drive(&mut self) -> Result<Vec<Frame>, H264Error> {
            let mut frames = Vec::new();
            loop {
                self.ensure_media_types()?;
                let mut progressed = self.flush_ready_input()?;
                let pulled = self.pull_output()?;
                progressed |= !pulled.is_empty();
                frames.extend(pulled);

                if !progressed {
                    if frames.is_empty() {
                        self.idle_pulls += 1;
                        if self.idle_pulls >= MAX_IDLE_PULLS {
                            return Err(H264Error::new(
                                "video decoder stalled: no frames after ".to_string()
                                    + &MAX_IDLE_PULLS.to_string()
                                    + " access units",
                            ));
                        }
                    } else {
                        self.idle_pulls = 0;
                    }
                    break;
                }
                if frames.len() >= MAX_IDLE_PULLS as usize {
                    break;
                }
            }
            if !frames.is_empty() {
                self.idle_pulls = 0;
            }
            Ok(frames)
        }

        /// Negotiate the media types, in two phases.
        ///
        /// The input type is set before the geometry is known: the MFT learns
        /// it from the bitstream (SPS) and reports it back on the output media
        /// type. The output type is therefore only negotiated once the MFT has
        /// actually accepted input — offering `SetOutputType` earlier is
        /// rejected with `E_INVALIDARG` because the decoder cannot describe a
        /// picture it has not seen yet. `build_input_type` omits
        /// `MF_MT_FRAME_SIZE` while `dimensions` is `None`.
        fn ensure_media_types(&mut self) -> Result<(), H264Error> {
            // The one ordering the SDK states outright: the device manager
            // goes in before the types, and both streaming notifications go
            // out before the first sample is processed
            // (`Mftransform.idl`: "send by pipeline before processing the
            // first sample"). The decoder deadlocked in `ProcessOutput` with
            // the notifications sent after `ProcessInput` #1, so they now
            // lead the sample.
            self.bind_device();
            self.request_frame_pool();
            if !self.input_set {
                unsafe {
                    self.transform
                        .SetInputType(0, &self.build_input_type()?, 0)
                        .map_err(|e| Self::error("setting the decoder input type", e))?;
                }
                self.input_set = true;
                unsafe {
                    self.transform
                        .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                        .map_err(|e| Self::error("starting the decoder stream", e))?;
                    self.transform
                        .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                        .map_err(|e| Self::error("starting the decoder stream", e))?;
                }
            }
            if self.input_set && !self.output_set && self.sample_index > 0 {
                self.request_frame_pool();
                let (output_type, device_type) = self.negotiate_output_type()?;
                unsafe {
                    self.transform
                        .SetOutputType(0, &output_type, 0)
                        .map_err(|e| Self::error("setting the decoder output type", e))?;
                }
                let dimensions = type_dimensions(&output_type, self.dimensions)?;
                self.dimensions = Some(dimensions);
                self.geometry = Some(Geometry {
                    coded: dimensions,
                    visible: visible_area(&output_type).unwrap_or(dimensions),
                });
                self.hardware = device_type;
                Self::debug_log(format!(
                    "output type negotiated ({dimensions:?}, hardware={})",
                    self.hardware
                ));
                if self.hardware && self.copier.is_none() {
                    let device = self
                        .device
                        .clone()
                        .ok_or_else(|| H264Error::new("the decoder lost its device"))?;
                    self.copier = Some(Copier::new(device, dimensions).map_err(H264Error::new)?);
                }
                self.output_set = true;
                self.request_frame_pool();
            }
            Ok(())
        }

        fn build_input_type(&self) -> Result<IMFMediaType, H264Error> {
            let media_type = unsafe {
                MFCreateMediaType().map_err(|e| Self::error("creating a media type", e))?
            };
            unsafe {
                media_type
                    .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                    .map_err(|e| Self::error("describing the decoder input", e))?;
                media_type
                    .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
                    .map_err(|e| Self::error("describing the decoder input", e))?;
            }
            let dimensions = self.dimensions;
            if let Some((width, height)) = dimensions {
                unsafe {
                    media_type
                        .SetUINT64(&MF_MT_FRAME_SIZE, pack_frame_size(width, height))
                        .map_err(|e| Self::error("describing the decoder input", e))?;
                }
            }
            Ok(media_type)
        }

        /// Pick the decoder's output type: NV12 first, then RGB32, falling
        /// back to whatever the MFT offers.
        ///
        /// A live stream re-offers its types when the picture changes, which
        /// the MFT reports as `MF_E_TRANSFORM_STREAM_CHANGE`; `pull_output`
        /// clears the negotiated flags so this runs again.
        fn negotiate_output_type(&self) -> Result<(IMFMediaType, bool), H264Error> {
            let mut offered: Vec<IMFMediaType> = Vec::new();
            let mut index = 0u32;
            while let Ok(media_type) = unsafe { self.transform.GetOutputAvailableType(0, index) } {
                offered.push(media_type);
                index += 1;
                if index >= MAX_OFFERED_OUTPUT_TYPES {
                    break;
                }
            }

            if offered.is_empty() {
                return Err(H264Error::new(
                    "the H.264 decoder MFT offered no output media type",
                ));
            }

            let nv12 = offered
                .iter()
                .find(|media_type| subtype(media_type).ok() == Some(MFVideoFormat_NV12));

            // Device textures only exist in NV12, so a decoder that took the
            // device and does not offer NV12 cannot decode into textures.
            let device_type =
                self.hardware_requested() && nv12.is_some() && provides_samples(&self.transform);

            let preferred = if device_type {
                nv12.expect("checked")
            } else {
                nv12.or_else(|| {
                    offered
                        .iter()
                        .find(|media_type| subtype(media_type).ok() == Some(MFVideoFormat_RGB32))
                })
                .unwrap_or(&offered[0])
            };

            // A type taken from the decoder still has to be cloned: the MFT
            // writes into the instance it is given.
            Ok((copy_media_type(preferred)?, device_type))
        }

        /// Hand every ready access unit to the MFT.
        fn flush_ready_input(&mut self) -> Result<bool, H264Error> {
            let mut progressed = false;
            while self.input_set && !self.ready.is_empty() {
                let access_unit = self.ready.remove(0);
                let sample = self.build_sample(&access_unit)?;
                unsafe {
                    self.transform
                        .ProcessInput(0, &sample, 0)
                        .map_err(|e| Self::error("feeding the decoder", e))?;
                }
                self.sample_index += 1;
                Self::debug_log(format!(
                    "fed access unit {} to the decoder",
                    self.sample_index
                ));
                progressed = true;
            }
            Ok(progressed)
        }

        fn build_sample(&self, access_unit: &[u8]) -> Result<IMFSample, H264Error> {
            let length = u32::try_from(access_unit.len()).map_err(|_| {
                H264Error::new("access unit too large for the decoder input buffer")
            })?;
            let buffer =
                unsafe { MFCreateMemoryBuffer(length).map_err(|e| Self::error("allocating", e))? };

            let mut target: *mut u8 = std::ptr::null_mut();
            unsafe {
                buffer
                    .Lock(&mut target, None, None)
                    .map_err(|e| Self::error("locking the decoder input buffer", e))?;
            }
            if target.is_null() {
                unsafe {
                    let _ = buffer.Unlock();
                }
                return Err(H264Error::new("the decoder input buffer locked to null"));
            }
            unsafe {
                std::ptr::copy_nonoverlapping(access_unit.as_ptr(), target, access_unit.len());
                buffer
                    .Unlock()
                    .map_err(|e| Self::error("unlocking the decoder input buffer", e))?;
                buffer
                    .SetCurrentLength(length)
                    .map_err(|e| Self::error("sizing the decoder input buffer", e))?;
            }

            let sample =
                unsafe { MFCreateSample().map_err(|e| Self::error("creating a sample", e))? };
            unsafe {
                sample
                    .AddBuffer(&buffer)
                    .map_err(|e| Self::error("attaching the decoder input buffer", e))?;
                sample
                    .SetSampleTime(self.sample_index * FRAME_DURATION)
                    .map_err(|e| Self::error("timestamping the decoder input", e))?;
                sample
                    .SetSampleDuration(FRAME_DURATION)
                    .map_err(|e| Self::error("timestamping the decoder input", e))?;
            }
            Ok(sample)
        }

        /// Turn one decoded sample buffer into a frame.
        ///
        /// In device mode the buffer holds a texture: it is copied into the
        /// decode-side ring straight away, so the decoder can recycle its own
        /// texture while the window thread works on the copy.
        fn frame(
            &mut self,
            buffer: &IMFMediaBuffer,
            media_type: &IMFMediaType,
            coded_width: u32,
            coded_height: u32,
        ) -> Result<Frame, H264Error> {
            if !self.hardware {
                return convert_buffer(buffer, media_type, coded_width, coded_height);
            }
            let geometry = self
                .geometry
                .ok_or_else(|| H264Error::new("the decoder has no negotiated output geometry"))?;
            let decoded = match gpu::texture_from_buffer(buffer) {
                Ok(decoded) => decoded,
                Err(error) => {
                    self.retry_or_degrade(error);
                    return Err(H264Error::new("the decoder lost its device textures"));
                }
            };
            let copier = self
                .copier
                .as_mut()
                .ok_or_else(|| H264Error::new("the decoder lost its copy ring"))?;
            let copied = copier.copy(&decoded.texture, decoded.subresource);
            Ok(Frame::Texture {
                texture: copied,
                coded: geometry.coded,
                visible: geometry.visible,
            })
        }

        /// Tell a device decoder how big its frame pool is.
        ///
        /// A DXVA decoder hands out textures from a pool it owns and blocks
        /// until one is handed back, so the count has to be on the output
        /// stream attributes *before* the decoder builds that pool — it does
        /// so while the output type is negotiated. Writing the attribute later
        /// leaves the pool empty and the first `ProcessOutput` never returns.
        fn request_frame_pool(&self) {
            if self.manager.is_none() {
                return;
            }
            let count = STREAMING_SAMPLE_COUNT;
            if let Ok(attributes) = unsafe { self.transform.GetOutputStreamAttributes(0) } {
                unsafe {
                    let _ = attributes.SetUINT32(&MF_SA_MINIMUM_OUTPUT_SAMPLE_COUNT, count);
                    let _ =
                        attributes.SetUINT32(&MF_SA_MINIMUM_OUTPUT_SAMPLE_COUNT_PROGRESSIVE, count);
                    // The decoder's own output textures are sized and bound
                    // from this while the output type is negotiated: they
                    // have to be bindable as decoder targets.
                    let _ =
                        attributes.SetUINT32(&MF_SA_D3D11_BINDFLAGS, D3D11_BIND_DECODER.0 as u32);
                }
            }
            // The decoder may size its pool from its own attribute store
            // rather than from the stream's, so both are filled.
            if let Ok(attributes) = unsafe { self.transform.GetAttributes() } {
                unsafe {
                    let _ = attributes.SetUINT32(&MF_SA_MINIMUM_OUTPUT_SAMPLE_COUNT, count);
                }
            }
        }

        /// Pull whatever the decoder has ready.
        fn pull_output(&mut self) -> Result<Vec<Frame>, H264Error> {
            if !self.output_set {
                return Ok(Vec::new());
            }

            let (coded_width, coded_height) = self
                .dimensions
                .ok_or_else(|| H264Error::new("the decoder has no negotiated output geometry"))?;
            // The frame layout (visible size, row pitch, pixel type) comes
            // from the negotiated output type, so it is read once per pull
            // rather than assumed: `STREAM_CHANGE` breaks out of the loop
            // below and the next pull reads the renegotiated type.
            let media_type = unsafe { self.transform.GetOutputCurrentType(0) }
                .map_err(|e| Self::error("reading the decoder output type", e))?;

            let mut frames = Vec::new();
            loop {
                let (buffer, partial) = match self.process_output()? {
                    Some(result) => result,
                    None => break,
                };

                frames.push(self.frame(&buffer, &media_type, coded_width, coded_height)?);

                if partial {
                    // The MFT reported `MFT_OUTPUT_DATA_BUFFER_INCOMPLETE`:
                    // the picture is not finished.
                    break;
                }
            }
            Ok(frames)
        }

        /// One `ProcessOutput` call, feeding the decoder an empty output sample
        /// it can write one frame into. `Ok(None)` means the decoder wants
        /// more input.
        fn process_output(&mut self) -> Result<Option<(IMFMediaBuffer, bool)>, H264Error> {
            let (width, height) = self
                .dimensions
                .ok_or_else(|| H264Error::new("the decoder has no negotiated output geometry"))?;
            // A decoder working on device textures allocates its own output
            // sample: handing it one would ask for system memory instead.
            let sample = if self.hardware {
                None
            } else {
                Some(self.build_output_sample(width, height)?)
            };

            let mut output = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(sample),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };
            let mut status = 0u32;

            Self::debug_log("ProcessOutput: asking the decoder for a frame");
            let result = {
                let outputs = std::slice::from_mut(&mut output);
                unsafe { self.transform.ProcessOutput(0, outputs, &mut status) }
            };

            // The decoder borrows the sample rather than handing back one of
            // its own, so it has to be reclaimed on every path: `ManuallyDrop`
            // never drops it and the reference is ours to release.
            let sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
            let events = unsafe { ManuallyDrop::take(&mut output.pEvents) };
            drop(events);

            match result {
                Ok(()) => {}
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(None),
                Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    // The picture changed (rotation, resolution): the decoder
                    // now offers a different output type, so the output type
                    // has to be negotiated again. The input type stays as it
                    // is: re-setting it makes the decoder forget the geometry
                    // it learned from the bitstream and offer its generic
                    // 1920x1080 type again, which lands back here forever and
                    // no frame is ever produced.
                    self.output_set = false;
                    return Ok(None);
                }
                Err(error) => {
                    let why = format!("taking a decoded frame: {error}");
                    if self.hardware {
                        // Never fatal: the decode side drops back to system
                        // memory and the next push renegotiates.
                        self.retry_or_degrade(why);
                        return Ok(None);
                    }
                    return Err(H264Error::new(why));
                }
            }

            let partial = (output.dwStatus & MFT_OUTPUT_DATA_BUFFER_INCOMPLETE.0 as u32) != 0;

            let Some(sample) = sample else {
                if partial {
                    return Ok(None);
                }
                return Err(H264Error::new(
                    "the decoder reported a frame without a sample",
                ));
            };

            // A texture sample holds its picture in a DXGI buffer, which is
            // not contiguous system memory; a memory sample is.
            let buffer = if self.hardware {
                match unsafe { sample.GetBufferByIndex(0) } {
                    Ok(buffer) => buffer,
                    Err(error) => {
                        let why = format!("taking the decoded texture: {error}");
                        self.retry_or_degrade(why);
                        return Ok(None);
                    }
                }
            } else {
                match unsafe { sample.ConvertToContiguousBuffer() } {
                    Ok(buffer) => buffer,
                    Err(error) => {
                        buffer_length(&sample)
                            .map_err(|e| Self::error("reading a decoded frame", e))?;
                        return Err(Self::error("reading a decoded frame", error));
                    }
                }
            };
            Ok(Some((buffer, partial)))
        }

        /// Allocate the empty output sample `ProcessOutput` writes a frame
        /// into.
        ///
        /// The H.264 decoder MFT does not provide output samples of its own:
        /// `ProcessOutput` requires the caller to pass an `IMFSample` holding
        /// a writable `IMFMediaBuffer` sized for the negotiated output type,
        /// and rejects a null `pSample` with `E_INVALIDARG` (`0x80070057`)
        /// before giving `MF_E_TRANSFORM_NEED_MORE_INPUT` a chance to mean
        /// anything.
        fn build_output_sample(&self, width: u32, height: u32) -> Result<IMFSample, H264Error> {
            let length = self.output_buffer_length(width, height)?;
            let buffer = unsafe {
                MFCreateMemoryBuffer(length)
                    .map_err(|e| Self::error("allocating the decoder output buffer", e))?
            };
            let sample = unsafe {
                MFCreateSample()
                    .map_err(|e| Self::error("creating the decoder output sample", e))?
            };
            unsafe {
                sample
                    .AddBuffer(&buffer)
                    .map_err(|e| Self::error("attaching the decoder output buffer", e))?;
            }
            Ok(sample)
        }

        /// Bytes the decoder needs for one frame of the negotiated output
        /// type: it reports the exact figure itself, and the geometrical
        /// worst case stands in when it reports none.
        fn output_buffer_length(&self, width: u32, height: u32) -> Result<u32, H264Error> {
            if let Ok(info) = unsafe { self.transform.GetOutputStreamInfo(0) } {
                if info.cbSize > 0 {
                    return Ok(info.cbSize);
                }
            }
            // Four bytes per pixel covers every uncompressed type the decoder
            // offers; the extra row keeps a copied frame inside the buffer
            // whatever pitch the decoder writes with.
            let bytes = (width as u64 * height as u64 * 4).saturating_add(width as u64 * 4);
            u32::try_from(bytes)
                .map_err(|_| H264Error::new("the decoder output frame is too large to allocate"))
        }
    }

    /// The decoder reports whether it provides its own output samples, which
    /// is what a device-texture output type does.
    fn provides_samples(transform: &IMFTransform) -> bool {
        unsafe { transform.GetOutputStreamInfo(0) }
            .map(|info| (info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0)
            .unwrap_or(false)
    }

    fn type_dimensions(
        media_type: &IMFMediaType,
        fallback: Option<(u32, u32)>,
    ) -> Result<(u32, u32), H264Error> {
        match unpack_frame_size(media_type) {
            Some(size) => Ok(size),
            None => fallback.ok_or_else(|| {
                H264Error::new("the decoder output media type carries no frame size")
            }),
        }
    }

    fn unpack_frame_size(media_type: &IMFMediaType) -> Option<(u32, u32)> {
        let packed = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }.ok()?;
        let width = ((packed >> 32) & 0xFFFF_FFFF) as u32;
        let height = (packed & 0xFFFF_FFFF) as u32;
        if width == 0 || height == 0 {
            return None;
        }
        Some((width, height))
    }

    fn pack_frame_size(width: u32, height: u32) -> u64 {
        ((width as u64) << 32) | (height as u64)
    }

    fn subtype(media_type: &IMFMediaType) -> windows::core::Result<windows::core::GUID> {
        unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }
    }

    /// Clone an offered media type into a fresh instance for `SetOutputType`.
    fn copy_media_type(source: &IMFMediaType) -> Result<IMFMediaType, H264Error> {
        let copy = unsafe { MFCreateMediaType().map_err(copy_error)? };
        unsafe {
            source
                .CopyAllItems(&copy)
                .map_err(|_| H264Error::new("copying the decoder output media type"))?;
        }
        Ok(copy)
    }

    fn copy_error(error: windows::core::Error) -> H264Error {
        H264Error::new(format!("creating a media type: {error}"))
    }

    fn buffer_length(sample: &IMFSample) -> windows::core::Result<u32> {
        unsafe { sample.GetTotalLength() }
    }

    /// Copy a decoded buffer into a tightly packed BGRA frame.
    ///
    /// The frame is presented at the decoder's *visible* geometry: the
    /// negotiated output type carries the coded size in `MF_MT_FRAME_SIZE`
    /// (1616 rows for this stream) while its display aperture carries the
    /// cropped picture (1604 rows), and only the aperture is real content.
    fn convert_buffer(
        buffer: &IMFMediaBuffer,
        media_type: &IMFMediaType,
        coded_width: u32,
        coded_height: u32,
    ) -> Result<Frame, H264Error> {
        let (width, height) = visible_area(media_type).unwrap_or((coded_width, coded_height));
        if width == 0 || height == 0 {
            return Err(H264Error::new("the decoder output geometry is empty"));
        }

        let nv12 = subtype(media_type).ok() == Some(MFVideoFormat_NV12);
        // The row pitch the decoder writes with. It reports it on the media
        // type; without it the frame is assumed packed at the coded width.
        let stride = unsafe { media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.unwrap_or(0) as i32;
        if stride < 0 {
            return Err(H264Error::new(
                "the decoder writes bottom-up frames, which is not supported",
            ));
        }
        let pitch = match stride {
            0 if nv12 => coded_width as usize,
            0 => coded_width as usize * 4,
            stride => stride as usize,
        };

        let bytes = lock_bytes(buffer)?;
        let bgra = if nv12 {
            pixels::nv12_to_bgra(&bytes, pitch, coded_height, width, height)
                .map_err(H264Error::new)?
        } else {
            pixels::copy_rows(&bytes, pitch, width, height).map_err(H264Error::new)?
        };

        Ok(Frame::Bgra {
            width,
            height,
            stride: width * 4,
            bgra,
        })
    }

    /// The decoder's visible picture. `MF_MT_FRAME_SIZE` is the coded size,
    /// so the cropped area is what the display apertures carry; both are
    /// `MFVideoArea` blobs (offset X, offset Y, then a `SIZE` of two `i32`).
    fn visible_area(media_type: &IMFMediaType) -> Option<(u32, u32)> {
        [MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_GEOMETRIC_APERTURE]
            .iter()
            .find_map(|key| {
                let mut area = [0u8; 16];
                let mut written = 0u32;
                unsafe {
                    media_type
                        .GetBlob(key as *const _, &mut area, Some(&mut written))
                        .ok()?
                };
                let width = i32::from_le_bytes(area[8..12].try_into().ok()?);
                let height = i32::from_le_bytes(area[12..16].try_into().ok()?);
                (width > 0 && height > 0).then_some((width as u32, height as u32))
            })
    }

    /// Copy the decoder's buffer out of Media Foundation memory.
    fn lock_bytes(buffer: &IMFMediaBuffer) -> Result<Vec<u8>, H264Error> {
        let length = unsafe {
            buffer
                .GetCurrentLength()
                .map_err(|e| H264Error::new(format!("sizing a decoded frame: {e}")))?
        } as usize;
        let mut raw: *mut u8 = std::ptr::null_mut();
        unsafe {
            buffer
                .Lock(&mut raw, None, None)
                .map_err(|e| H264Error::new(format!("locking a decoded frame: {e}")))?;
        }
        if raw.is_null() {
            unsafe {
                let _ = buffer.Unlock();
            }
            return Err(H264Error::new("a decoded frame locked to null"));
        }
        let bytes = unsafe { std::slice::from_raw_parts(raw, length) }.to_vec();
        unsafe {
            buffer
                .Unlock()
                .map_err(|e| H264Error::new(format!("unlocking a decoded frame: {e}")))?;
        }
        Ok(bytes)
    }

    fn is_vcl(nal_type: u8) -> bool {
        nal_type == 1 || nal_type == 5
    }

    /// Find the next Annex-B start code at or after `from`.
    fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
        if from >= data.len() {
            return None;
        }
        let mut index = from;
        while index < data.len() {
            if index + START_CODE_4.len() <= data.len()
                && data[index..index + START_CODE_4.len()] == START_CODE_4
            {
                return Some((index, START_CODE_4.len()));
            }
            if index + START_CODE_3.len() <= data.len()
                && data[index..index + START_CODE_3.len()] == START_CODE_3
            {
                return Some((index, START_CODE_3.len()));
            }
            index += 1;
        }
        None
    }
}
