//! Direct3D 11 device, GPU colour conversion and window presentation.
//!
//! The mirror pipeline has two ends that need the same Direct3D device: the
//! H.264 decoder writes decoded pictures into device textures (through the
//! Media Foundation DXGI device manager) and the mirror window shows them.
//! This module owns that device, the ring of textures the decode thread copies
//! decoded pictures into, and the swap chain the window presents them on.
//!
//! [`Converter`] turns a picture into RGB with the fixed-function video
//! processor, scaled to the window: one GPU operation, no GDI stretch, no
//! per-pixel CPU work. [`Presenter`] owns the window's DXGI swap chain.
//! [`Device::read_bgra`] is the CPU escape hatch, used when the window cannot
//! present on the GPU at all.
//!
//! Nothing here is required for mirroring to work: the software pipeline in
//! [`crate::core::video`] decodes and paints without a device, so a machine
//! with no usable Direct3D adapter still mirrors.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::Arc;

use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D10::ID3D10Multithread;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, ID3D11VideoContext,
    ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView, D3D11_BIND_FLAG,
    D3D11_CPU_ACCESS_FLAG, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, D3D11_VIDEO_COLOR, D3D11_VIDEO_COLOR_0, D3D11_VIDEO_COLOR_RGBA,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_COLOR_SPACE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_UNSPECIFIED, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12,
    DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGIOutput, IDXGISwapChain1,
    DXGI_CREATE_FACTORY_FLAGS, DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFDXGIDeviceManager, IMFMediaBuffer, MFCreateDXGIDeviceManager,
};

use crate::core::pixels;

/// Feature levels offered when creating the device, newest first.
const FEATURE_LEVELS: [D3D_FEATURE_LEVEL; 4] = [
    D3D_FEATURE_LEVEL_11_1,
    D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_10_0,
];

/// Buffers in the swap chain: two is the minimum for the flip model, and it
/// bounds how far the GPU can fall behind the decode thread.
const SWAP_CHAIN_BUFFERS: u32 = 2;

/// Surfaces in the decode-side copy ring. At most three frames are in flight
/// (one being decoded, one in the slot, one being presented), so a deeper ring
/// cannot be recycled under a frame the window is still showing.
const RING: usize = 4;

/// A Direct3D 11 device with its video processing interfaces.
pub struct Device {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    adapter: String,
    hardware: bool,
}

impl Device {
    /// Create the device the mirror pipeline decodes and presents with.
    ///
    /// A hardware adapter is preferred; the software rasteriser is only used
    /// when no hardware adapter answers, and is reported through
    /// [`Device::is_hardware`] so the caller keeps hardware decoding off (the
    /// rasteriser has no video decoder).
    pub fn new() -> Result<Self, String> {
        let (device, hardware) = create_device()?;
        let context = unsafe { device.GetImmediateContext() }
            .map_err(|error| format!("no immediate device context: {error}"))?;
        let video_device = device
            .cast::<ID3D11VideoDevice>()
            .map_err(|error| format!("the adapter exposes no Direct3D 11 video device: {error}"))?;
        let video_context = context.cast::<ID3D11VideoContext>().map_err(|error| {
            format!("the adapter exposes no Direct3D 11 video context: {error}")
        })?;
        Ok(Self {
            adapter: adapter_name(&device),
            device,
            context,
            video_device,
            video_context,
            hardware,
        })
    }

    /// The adapter description, for logs and reports.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// True when the device runs on a hardware adapter.
    pub fn is_hardware(&self) -> bool {
        self.hardware
    }

    /// The device Media Foundation has to decode with.
    pub fn raw(&self) -> &ID3D11Device {
        &self.device
    }

    /// The immediate context, shared by the copy ring and the window thread.
    pub fn context(&self) -> &ID3D11DeviceContext {
        &self.context
    }

    /// Create a 2D texture with one mip level and one array slice.
    fn texture2d(
        &self,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        usage: D3D11_USAGE,
        bind: D3D11_BIND_FLAG,
        cpu: D3D11_CPU_ACCESS_FLAG,
    ) -> Result<ID3D11Texture2D, String> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width.max(1),
            Height: height.max(1),
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: usage,
            BindFlags: bind.0 as u32,
            CPUAccessFlags: cpu.0 as u32,
            MiscFlags: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut texture)) }
            .map_err(|error| format!("creating a {width}x{height} texture: {error}"))?;
        texture.ok_or_else(|| "creating a texture returned nothing".to_string())
    }

    /// Copy a decoded picture back to system memory as BGRA.
    ///
    /// The GPU pipeline presents without reading pixels on the CPU; this is
    /// the fallback for a window whose swap chain failed, and it is how the
    /// offline pipeline test compares GPU output with the software decoder.
    pub fn read_bgra(
        &self,
        source: &ID3D11Texture2D,
        subresource: u32,
        coded: (u32, u32),
        visible: (u32, u32),
    ) -> Result<Vec<u8>, String> {
        let staging = self.texture2d(
            coded.0,
            coded.1,
            DXGI_FORMAT_NV12,
            D3D11_USAGE_STAGING,
            D3D11_BIND_FLAG(0),
            D3D11_CPU_ACCESS_READ,
        )?;
        unsafe {
            self.context
                .CopySubresourceRegion(&staging, 0, 0, 0, 0, source, subresource, None)
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|error| format!("mapping the staging texture: {error}"))?;

        let result = if mapped.pData.is_null() {
            Err("the staging texture mapped to null".to_string())
        } else {
            let pitch = mapped.RowPitch as usize;
            // An NV12 allocation spans both planes: one luma row per coded
            // row, plus half as many chroma rows.
            let length = pitch * (coded.1 as usize + coded.1 as usize / 2);
            let bytes = unsafe { std::slice::from_raw_parts(mapped.pData as *const u8, length) };
            pixels::nv12_to_bgra(bytes, pitch, coded.1, visible.0, visible.1)
        };

        unsafe { self.context.Unmap(&staging, 0) };
        result
    }
}

/// Create a device, preferring a hardware adapter.
fn create_device() -> Result<(ID3D11Device, bool), String> {
    let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
    let attempts = [
        (D3D_DRIVER_TYPE_HARDWARE, true),
        (D3D_DRIVER_TYPE_WARP, false),
    ];
    let mut failure = String::new();
    for (driver, hardware) in attempts {
        let mut device: Option<ID3D11Device> = None;
        let mut level = D3D_FEATURE_LEVEL_10_0;
        let result = unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                driver,
                HMODULE::default(),
                flags,
                Some(&FEATURE_LEVELS),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut level),
                None,
            )
        };
        match (result, device) {
            (Ok(()), Some(device)) => {
                // Media Foundation drives the device from its own decode
                // threads while this process submits and presents on others,
                // so the device has to accept calls from more than one thread:
                // without this an MFT-bound device makes Direct3D raise on the
                // second thread.
                if let Ok(multithread) = device.cast::<ID3D10Multithread>() {
                    let _ = unsafe { multithread.SetMultithreadProtected(true) };
                }
                return Ok((device, hardware));
            }
            (Err(error), _) => failure = format!("{driver:?}: {error}"),
            (Ok(()), None) => failure = format!("{driver:?}: no device"),
        }
    }
    Err(format!("no Direct3D 11 device ({failure})"))
}

/// The adapter name behind a device, when the driver reports one.
fn adapter_name(device: &ID3D11Device) -> String {
    let described = || -> windows::core::Result<String> {
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter = unsafe { dxgi.GetAdapter() }?;
        let desc = unsafe { adapter.GetDesc() }?;
        let name: String = desc
            .Description
            .iter()
            .take_while(|unit| **unit != 0)
            .map(|unit| char::from_u32(*unit as u32).unwrap_or('?'))
            .collect();
        Ok(name)
    };
    match described() {
        Ok(name) if !name.is_empty() => name,
        _ => "unknown adapter".to_string(),
    }
}

/// The Media Foundation DXGI device manager the decoder is bound through.
///
/// Media Foundation keeps the manager alive as long as the decoder MFT is
/// bound to it, so the decoder state has to hold on to it. No COM interface is
/// marked `Send` (the reference is a bare pointer) but this one only ever
/// travels with the decoder thread that created it, and it is only ever used
/// from that thread: the wrapper states that invariant for the compiler.
pub struct DeviceManager(IMFDXGIDeviceManager);

unsafe impl Send for DeviceManager {}

impl DeviceManager {
    /// The interface itself, to hand to Media Foundation.
    pub fn raw(&self) -> &IMFDXGIDeviceManager {
        &self.0
    }
}

/// Media Foundation device manager for a device: binds the decoder to it so
/// decoded pictures land in device textures.
pub fn device_manager(device: &Device) -> Result<DeviceManager, String> {
    let mut token = 0u32;
    let mut manager = None;
    unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager) }
        .map_err(|error| format!("creating the DXGI device manager: {error}"))?;
    let manager = manager.ok_or_else(|| "no DXGI device manager".to_string())?;
    unsafe { manager.ResetDevice(device.raw(), token) }
        .map_err(|error| format!("binding the device to its manager: {error}"))?;
    Ok(DeviceManager(manager))
}

/// A picture living in one of our own device textures.
pub struct Texture {
    pub texture: ID3D11Texture2D,
    pub subresource: u32,
}

/// A ring of device textures the decode thread copies each decoded picture
/// into.
///
/// Media Foundation recycles the textures it decodes into, so a picture is
/// copied once, immediately after `ProcessOutput`, while the same thread
/// cannot yet have asked the decoder for another frame: the copy is ordered
/// before the decoder's next write on the device context. The window thread
/// then works on the copy and the decoder keeps recycling its own textures.
pub struct Copier {
    device: Arc<Device>,
    surfaces: Vec<ID3D11Texture2D>,
    next: usize,
    coded: (u32, u32),
}

impl Copier {
    pub fn new(device: Arc<Device>, coded: (u32, u32)) -> Result<Self, String> {
        let mut surfaces = Vec::with_capacity(RING);
        for _ in 0..RING {
            surfaces.push(device.texture2d(
                coded.0,
                coded.1,
                DXGI_FORMAT_NV12,
                D3D11_USAGE_DEFAULT,
                D3D11_BIND_FLAG(0),
                D3D11_CPU_ACCESS_FLAG(0),
            )?);
        }
        Ok(Self {
            device,
            surfaces,
            next: 0,
            coded,
        })
    }

    /// The coded size of the ring surfaces.
    pub fn coded(&self) -> (u32, u32) {
        self.coded
    }

    /// Copy one decoded picture into the next ring surface.
    pub fn copy(&mut self, source: &ID3D11Texture2D, subresource: u32) -> Texture {
        let index = self.next;
        self.next = (self.next + 1) % self.surfaces.len();
        let texture = self.surfaces[index].clone();
        unsafe {
            self.device.context.CopySubresourceRegion(
                &texture,
                0,
                0,
                0,
                0,
                source,
                subresource,
                None,
            )
        };
        Texture {
            texture,
            subresource: 0,
        }
    }
}

/// NV12 → RGB conversion for one picture geometry, on the GPU.
pub struct Converter {
    device: Arc<Device>,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    /// Input views of the pictures converted, keyed by their address (the
    /// decode thread rotates between a few ring surfaces).
    input_views: RefCell<HashMap<usize, ID3D11VideoProcessorInputView>>,
    /// Input views of the output surfaces, keyed the same way (a swap chain
    /// rotates between its buffers).
    output_views: RefCell<HashMap<usize, ID3D11VideoProcessorOutputView>>,
    /// The visible picture: the source rectangle taken from each input.
    geometry: (u32, u32),
    output: (u32, u32),
    /// The BGRA input path, built on first use: a frame that comes back in
    /// system memory still goes through the GPU when a device exists.
    bgra: Option<BgraPath>,
}

/// The BGRA input path of a converter.
///
/// It carries its own video processor: an enumerator is created for one input
/// format, and drivers are not required to mix a YUV and an RGB input on the
/// same processor.
struct BgraPath {
    processor: ID3D11VideoProcessor,
    texture: ID3D11Texture2D,
    view: ID3D11VideoProcessorInputView,
    size: (u32, u32),
}

impl Converter {
    /// Build the converter for `geometry` (the visible picture) writing into
    /// surfaces of `output` size.
    pub fn new(
        device: Arc<Device>,
        geometry: (u32, u32),
        output: (u32, u32),
    ) -> Result<Self, String> {
        let (enumerator, processor) = video_processor(&device, geometry, output)?;
        Ok(Self {
            device,
            enumerator,
            processor,
            input_views: RefCell::new(HashMap::new()),
            output_views: RefCell::new(HashMap::new()),
            geometry,
            output,
            bgra: None,
        })
    }

    /// The picture geometry the converter handles.
    pub fn geometry(&self) -> (u32, u32) {
        self.geometry
    }

    /// Convert one decoded NV12 picture onto `output`, filling `target`.
    pub fn convert(
        &mut self,
        source: &ID3D11Texture2D,
        subresource: u32,
        output: &ID3D11Texture2D,
        target: RECT,
    ) -> Result<(), String> {
        let view = self.input_view(source, subresource)?;
        let output_view = self.output_view(output)?;
        let source_rect = source_rect(self.geometry);
        unsafe {
            self.device
                .video_context
                .VideoProcessorSetStreamFrameFormat(
                    &self.processor,
                    0,
                    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                );
            self.device.video_context.VideoProcessorSetStreamSourceRect(
                &self.processor,
                0,
                true,
                Some(&source_rect),
            );
            self.device.video_context.VideoProcessorSetStreamColorSpace(
                &self.processor,
                0,
                &yuv_color_space(),
            );
        }
        blit(&self.device, &self.processor, &view, &output_view, target)
    }

    /// Convert a BGRA picture from system memory onto `output`.
    pub fn convert_bgra(
        &mut self,
        bytes: &[u8],
        size: (u32, u32),
        output: &ID3D11Texture2D,
        target: RECT,
    ) -> Result<(), String> {
        let expected = size.0 as usize * 4 * size.1 as usize;
        if bytes.len() < expected {
            return Err(format!(
                "a BGRA frame is short: {} bytes for {}x{}",
                bytes.len(),
                size.0,
                size.1
            ));
        }
        if self.bgra.as_ref().map(|path| path.size) != Some(size) {
            self.bgra = Some(BgraPath::new(&self.device, size)?);
        }
        let path = self.bgra.as_ref().expect("just built");
        unsafe {
            self.device.context.UpdateSubresource(
                &path.texture,
                0,
                None,
                bytes.as_ptr().cast(),
                size.0 * 4,
                0,
            );
            self.device
                .video_context
                .VideoProcessorSetStreamFrameFormat(
                    &path.processor,
                    0,
                    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                );
            self.device.video_context.VideoProcessorSetStreamSourceRect(
                &path.processor,
                0,
                true,
                Some(&source_rect(size)),
            );
            // BGRA frames come from the software decoder already in display
            // range, so the input is full-range RGB.
            self.device.video_context.VideoProcessorSetStreamColorSpace(
                &path.processor,
                0,
                &rgb_color_space(),
            );
        }
        let output_view = self.output_view(output)?;
        let processor = path.processor.clone();
        let view = path.view.clone();
        blit(&self.device, &processor, &view, &output_view, target)
    }

    fn input_view(
        &self,
        source: &ID3D11Texture2D,
        subresource: u32,
    ) -> Result<ID3D11VideoProcessorInputView, String> {
        let key = source.as_raw() as usize ^ subresource as usize;
        if let Some(view) = self.input_views.borrow().get(&key) {
            return Ok(view.clone());
        }
        let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: subresource,
                },
            },
        };
        let mut view: Option<ID3D11VideoProcessorInputView> = None;
        unsafe {
            self.device.video_device.CreateVideoProcessorInputView(
                source,
                &self.enumerator,
                &desc,
                Some(&mut view),
            )
        }
        .map_err(|error| format!("creating the video processor input view: {error}"))?;
        let view = view.ok_or_else(|| "creating an input view returned nothing".to_string())?;
        self.input_views.borrow_mut().insert(key, view.clone());
        Ok(view)
    }

    fn output_view(
        &self,
        output: &ID3D11Texture2D,
    ) -> Result<ID3D11VideoProcessorOutputView, String> {
        let key = output.as_raw() as usize;
        if let Some(view) = self.output_views.borrow().get(&key) {
            return Ok(view.clone());
        }
        let desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut view: Option<ID3D11VideoProcessorOutputView> = None;
        unsafe {
            self.device.video_device.CreateVideoProcessorOutputView(
                output,
                &self.enumerator,
                &desc,
                Some(&mut view),
            )
        }
        .map_err(|error| format!("creating the video processor output view: {error}"))?;
        let view = view.ok_or_else(|| "creating an output view returned nothing".to_string())?;
        self.output_views.borrow_mut().insert(key, view.clone());
        Ok(view)
    }
}

impl BgraPath {
    fn new(device: &Arc<Device>, size: (u32, u32)) -> Result<Self, String> {
        let texture = device.texture2d(
            size.0,
            size.1,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            D3D11_USAGE_DEFAULT,
            D3D11_BIND_FLAG(0),
            D3D11_CPU_ACCESS_FLAG(0),
        )?;
        // The enumerator describes the conversion for the picture size; the
        // input format is a property of the view.
        let (enumerator, processor) = video_processor(device, size, size)?;
        let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut view: Option<ID3D11VideoProcessorInputView> = None;
        unsafe {
            device.video_device.CreateVideoProcessorInputView(
                &texture,
                &enumerator,
                &desc,
                Some(&mut view),
            )
        }
        .map_err(|error| format!("creating the BGRA input view: {error}"))?;
        let view = view.ok_or_else(|| "creating a BGRA input view returned nothing".to_string())?;
        Ok(Self {
            processor,
            texture,
            view,
            size,
        })
    }
}

/// One video processor for a picture geometry.
fn video_processor(
    device: &Arc<Device>,
    geometry: (u32, u32),
    output: (u32, u32),
) -> Result<(ID3D11VideoProcessorEnumerator, ID3D11VideoProcessor), String> {
    let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
        InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        InputFrameRate: frame_rate(),
        InputWidth: geometry.0.max(1),
        InputHeight: geometry.1.max(1),
        OutputFrameRate: frame_rate(),
        OutputWidth: output.0.max(1),
        OutputHeight: output.1.max(1),
        Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
    };
    let enumerator = unsafe { device.video_device.CreateVideoProcessorEnumerator(&content) }
        .map_err(|error| format!("no video processor for {geometry:?} into {output:?}: {error}"))?;
    let processor = unsafe { device.video_device.CreateVideoProcessor(&enumerator, 0) }
        .map_err(|error| format!("creating the video processor: {error}"))?;
    Ok((enumerator, processor))
}

/// One video processor operation: scale and convert the input onto `target`.
fn blit(
    device: &Arc<Device>,
    processor: &ID3D11VideoProcessor,
    input: &ID3D11VideoProcessorInputView,
    output: &ID3D11VideoProcessorOutputView,
    target: RECT,
) -> Result<(), String> {
    unsafe {
        device
            .video_context
            .VideoProcessorSetOutputBackgroundColor(processor, false, &black());
        device
            .video_context
            .VideoProcessorSetOutputTargetRect(processor, true, Some(&target));
    }
    let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
        Enable: true.into(),
        pInputSurface: ManuallyDrop::new(Some(input.clone())),
        ..Default::default()
    };
    let result = unsafe {
        device
            .video_context
            .VideoProcessorBlt(processor, output, 0, std::slice::from_ref(&stream))
    };
    // The video processor only borrows the surface: releasing the reference
    // the stream holds keeps the refcount honest, one per blit.
    unsafe { ManuallyDrop::drop(&mut stream.pInputSurface) };
    result.map_err(|error| format!("converting the picture on the GPU: {error}"))
}

/// The mirror window's swap chain: presents converted pictures.
pub struct Presenter {
    converter: Converter,
    swap_chain: IDXGISwapChain1,
    client: (u32, u32),
}

impl Presenter {
    /// Create the swap chain and converter for a window.
    pub fn new(
        device: Arc<Device>,
        hwnd: HWND,
        client: (u32, u32),
        geometry: (u32, u32),
    ) -> Result<Self, String> {
        let swap_chain = create_swap_chain(&device, hwnd, client)?;
        let converter = Converter::new(device, geometry, client)?;
        Ok(Self {
            converter,
            swap_chain,
            client,
        })
    }

    /// The picture geometry the presenter was built for.
    pub fn geometry(&self) -> (u32, u32) {
        self.converter.geometry()
    }

    /// Show one decoded picture: convert onto the back buffer and present.
    pub fn present(&mut self, texture: &ID3D11Texture2D, subresource: u32) -> Result<(), String> {
        let target = fitted_rect(self.converter.geometry(), self.client);
        let back_buffer: ID3D11Texture2D = unsafe { self.swap_chain.GetBuffer(0) }
            .map_err(|error| format!("taking the back buffer: {error}"))?;
        self.converter
            .convert(texture, subresource, &back_buffer, target)?;
        self.swap()
    }

    /// Show one BGRA picture held in system memory.
    pub fn present_bgra(&mut self, bytes: &[u8], size: (u32, u32)) -> Result<(), String> {
        let target = fitted_rect(size, self.client);
        let back_buffer: ID3D11Texture2D = unsafe { self.swap_chain.GetBuffer(0) }
            .map_err(|error| format!("taking the back buffer: {error}"))?;
        self.converter
            .convert_bgra(bytes, size, &back_buffer, target)?;
        self.swap()
    }

    fn swap(&self) -> Result<(), String> {
        // Present with vsync: it paces the pipeline to the display instead of
        // letting the queue grow.
        unsafe { self.swap_chain.Present(1, DXGI_PRESENT(0)) }
            .ok()
            .map_err(|error| format!("presenting the frame: {error}"))
    }

    /// Follow a client area change.
    pub fn resize(&mut self, client: (u32, u32)) -> Result<(), String> {
        if client == self.client {
            return Ok(());
        }
        unsafe {
            self.swap_chain.ResizeBuffers(
                SWAP_CHAIN_BUFFERS,
                client.0.max(1),
                client.1.max(1),
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG(0),
            )
        }
        .map_err(|error| format!("resizing the swap chain: {error}"))?;
        // The new buffers need their own output views, and the video processor
        // is sized for the surface it writes into.
        self.client = client;
        self.converter.output = client;
        self.converter.output_views.borrow_mut().clear();
        Ok(())
    }
}

/// A swap chain for one window, in the flip model (no GDI interference).
fn create_swap_chain(
    device: &Device,
    hwnd: HWND,
    client: (u32, u32),
) -> Result<IDXGISwapChain1, String> {
    let factory: IDXGIFactory2 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }
        .map_err(|error| format!("no DXGI factory: {error}"))?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: client.0.max(1),
        Height: client.1.max(1),
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: SWAP_CHAIN_BUFFERS,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
        Flags: 0,
    };
    let chain = unsafe {
        factory.CreateSwapChainForHwnd(device.raw(), hwnd, &desc, None, None::<&IDXGIOutput>)
    }
    .map_err(|error| {
        format!(
            "creating a swap chain for a {}x{} window: {error}",
            client.0, client.1
        )
    })?;
    // Alt+Enter must not take the mirror window full screen.
    let _ = unsafe { factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER) };
    Ok(chain)
}

/// Where the picture sits in the client area: fitted, centred, letterboxed.
fn fitted_rect(video: (u32, u32), client: (u32, u32)) -> RECT {
    let (client_w, client_h) = (client.0.max(1) as f64, client.1.max(1) as f64);
    let scale = f64::min(
        client_w / video.0.max(1) as f64,
        client_h / video.1.max(1) as f64,
    );
    let width = (video.0 as f64 * scale).round().max(1.0) as i32;
    let height = (video.1 as f64 * scale).round().max(1.0) as i32;
    let left = (client.0 as i32 - width) / 2;
    let top = (client.1 as i32 - height) / 2;
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

fn source_rect(size: (u32, u32)) -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: size.0.max(1) as i32,
        bottom: size.1.max(1) as i32,
    }
}

fn frame_rate() -> DXGI_RATIONAL {
    DXGI_RATIONAL {
        Numerator: 30,
        Denominator: 1,
    }
}

fn black() -> D3D11_VIDEO_COLOR {
    D3D11_VIDEO_COLOR {
        Anonymous: D3D11_VIDEO_COLOR_0 {
            RGBA: D3D11_VIDEO_COLOR_RGBA {
                R: 0.0,
                G: 0.0,
                B: 0.0,
                A: 1.0,
            },
        },
    }
}

/// The colour space of the NV12 the phone's encoder produces.
///
/// `D3D11_VIDEO_PROCESSOR_COLOR_SPACE` is a bit field — `Usage` bit 0,
/// `RGB_Range` bit 1, `YCbCr_Matrix` bit 2, `YCbCr_xvYCC` bit 3 and
/// `Nominal_Range` bits 4-5 — and the scrcpy stream is BT.601 studio swing
/// (`color_range=tv`, `color_space=bt470bg`), the assumption the software NV12
/// conversion in [`crate::core::pixels`] makes too.
fn yuv_color_space() -> D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
    const USAGE_PLAYBACK: u32 = 0;
    const RGB_STUDIO_SWING: u32 = 0;
    const YCBCR_MATRIX_BT601: u32 = 0;
    const XVYCC_OFF: u32 = 0;
    const NOMINAL_RANGE_16_235: u32 = 1;
    D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
        _bitfield: USAGE_PLAYBACK
            | (RGB_STUDIO_SWING << 1)
            | (YCBCR_MATRIX_BT601 << 2)
            | (XVYCC_OFF << 3)
            | (NOMINAL_RANGE_16_235 << 4),
    }
}

/// The colour space of the BGRA the software decoder produces.
fn rgb_color_space() -> D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
    const USAGE_PLAYBACK: u32 = 0;
    const RGB_FULL_RANGE: u32 = 1;
    const YCBCR_MATRIX_BT601: u32 = 0;
    const XVYCC_OFF: u32 = 0;
    const NOMINAL_RANGE_0_255: u32 = 0;
    D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
        _bitfield: USAGE_PLAYBACK
            | (RGB_FULL_RANGE << 1)
            | (YCBCR_MATRIX_BT601 << 2)
            | (XVYCC_OFF << 3)
            | (NOMINAL_RANGE_0_255 << 4),
    }
}

/// Take the Direct3D texture out of a decoded sample's buffer.
pub fn texture_from_buffer(buffer: &IMFMediaBuffer) -> Result<Texture, String> {
    let dxgi: IMFDXGIBuffer = buffer
        .cast()
        .map_err(|error| format!("the decoder did not decode into a texture: {error}"))?;
    let mut raw: *mut c_void = std::ptr::null_mut();
    unsafe { dxgi.GetResource(&ID3D11Texture2D::IID, &mut raw) }
        .map_err(|error| format!("taking the decoded texture: {error}"))?;
    if raw.is_null() {
        return Err("the decoder returned a null texture".to_string());
    }
    let texture = unsafe { ID3D11Texture2D::from_raw(raw) };
    let subresource = unsafe { dxgi.GetSubresourceIndex() }
        .map_err(|error| format!("reading the decoded subresource: {error}"))?;
    Ok(Texture {
        texture,
        subresource,
    })
}

/// Keep the compiler honest about what crosses the decode/window thread
/// boundary.
#[allow(dead_code)]
fn thread_boundary() {
    fn send<T: Send>() {}
    send::<Device>();
    send::<Texture>();
    send::<Copier>();
    send::<Presenter>();
    send::<DeviceManager>();
}
