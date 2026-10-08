//! Offline decode test for the H.264 decoder MFT.
//!
//! Set `HERMESGATE_H264_SAMPLE` to a raw Annex-B H.264 file and run:
//!
//! ```text
//! HERMESGATE_H264_SAMPLE=/path/raw.h264 cargo test --test h264_offline -- --nocapture
//! ```
//!
//! Without the variable the test is a no-op so the suite stays green on a
//! machine that has no sample. It is a harness, not a unit test: it asserts
//! the geometry and that the frames carry real pixels.
//!
//! Setting `HERMESGATE_H264_HW=1` additionally decodes the same sample through
//! the Direct3D 11 device path (`Decoder::new_hardware`), which is the only way
//! to exercise GPU decoding without a phone attached: the decoder reports
//! whether it took the device, and the frames it produces have to match the
//! software decoder's geometry.
//!
//! That test is `#[ignore]`d: it needs `HERMESGATE_H264_HW=1` plus a working
//! Direct3D 11 device, and if the decoder's message ordering ever regresses it
//! would hang `cargo test` instead of failing it. It now passes (the deadlock
//! was the SDK's one documented ordering rule - the streaming notifications
//! have to go out before the first sample, and `SET_D3D_MANAGER` before the
//! types; see ADR 0003), so run it with `--ignored` under a timeout as the
//! regression guard for that ordering.

use hermesgate_lib::core::h264::{Decoder, Frame};

/// Chunk size used to feed the stream, mimicking a socket read.
const CHUNK: usize = 8192;

/// The sample the tests decode, if one is configured.
fn sample() -> Option<(String, Vec<u8>)> {
    let path = std::env::var("HERMESGATE_H264_SAMPLE").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    eprintln!("sample {path}: {} bytes", bytes.len());
    Some((path, bytes))
}

/// Feed a whole stream through a decoder.
fn decode(decoder: &mut Decoder, bytes: &[u8]) -> Vec<Frame> {
    let mut frames = Vec::new();
    for chunk in bytes.chunks(CHUNK) {
        frames.extend(decoder.push(chunk).expect("feeding the decoder"));
    }
    frames.extend(decoder.finish().expect("draining the decoder"));
    frames
}

/// The BGRA pixels of a frame, reading a decoded texture back if the GPU
/// decoded it.
#[cfg(windows)]
fn pixels(frame: &Frame, device: &hermesgate_lib::core::gpu::Device) -> Vec<u8> {
    match frame {
        Frame::Bgra { bgra, .. } => bgra.clone(),
        Frame::Texture {
            texture,
            coded,
            visible,
        } => device
            .read_bgra(&texture.texture, texture.subresource, *coded, *visible)
            .expect("reading a decoded texture back"),
    }
}

/// Report one frame's contents in the shape the offline harness always has.
fn describe(index: usize, frame: &Frame, bgra: &[u8]) {
    let distinct = {
        let mut seen = [false; 256];
        for &byte in bgra.iter().step_by(97) {
            seen[byte as usize] = true;
        }
        seen.iter().filter(|&&s| s).count()
    };
    let off_base = bgra.iter().skip(1).filter(|&&b| b != bgra[0]).count();
    let (width, height) = frame.size();
    eprintln!(
        "  frame {index}: {width}x{height} len {} distinct {distinct} off-base {off_base}",
        bgra.len(),
    );
}

/// The assertions the harness makes about every decoded frame.
fn check(index: usize, size: (u32, u32), bgra: &[u8]) {
    assert_eq!(size, (720, 1604), "frame {index} geometry");
    assert_eq!(bgra.len(), 720 * 1604 * 4, "frame {index} BGRA length");
    assert!(
        bgra.iter().any(|&b| b != bgra[0]),
        "frame {index} is blank (one single byte value)"
    );
}

#[test]
fn decodes_sample_offline() {
    let Some((_path, bytes)) = sample() else {
        eprintln!("HERMESGATE_H264_SAMPLE is unset: skipping the offline decode test");
        return;
    };

    let mut decoder = Decoder::new().expect("creating the decoder");
    let frames = decode(&mut decoder, &bytes);

    eprintln!("decoded {} frames", frames.len());
    assert!(
        frames.len() >= 8,
        "expected at least 8 frames, got {}",
        frames.len()
    );

    for (index, frame) in frames.iter().enumerate() {
        let Frame::Bgra { bgra, .. } = frame else {
            panic!("frame {index}: a software decoder produced a device texture");
        };
        describe(index, frame, bgra);
        check(index, frame.size(), bgra);
    }
    eprintln!("software decode: {} frames verified", frames.len());
}

#[cfg(windows)]
#[test]
#[ignore = "needs HERMESGATE_H264_HW=1 and a D3D11 device; hangs cargo test if the message ordering regresses"]
fn decodes_sample_on_the_gpu() {
    if std::env::var("HERMESGATE_H264_HW").is_err() {
        eprintln!("HERMESGATE_H264_HW is unset: skipping the GPU decode test");
        return;
    }
    let Some((_path, bytes)) = sample() else {
        eprintln!("HERMESGATE_H264_SAMPLE is unset: skipping the GPU decode test");
        return;
    };
    use hermesgate_lib::core::gpu::Device;
    use std::sync::Arc;

    let device = Arc::new(Device::new().expect("creating the Direct3D device"));
    eprintln!(
        "device: {} (hardware: {})",
        device.adapter(),
        device.is_hardware()
    );

    let mut decoder = Decoder::new_hardware(device.clone()).expect("creating the GPU decoder");
    let frames = decode(&mut decoder, &bytes);
    eprintln!(
        "decoded {} frames, decoding {}",
        frames.len(),
        if decoder.is_hardware() {
            "on the GPU"
        } else {
            "in system memory"
        }
    );
    assert!(
        frames.len() >= 8,
        "expected at least 8 frames, got {}",
        frames.len()
    );
    assert_eq!(
        decoder.geometry().map(|geometry| geometry.visible),
        Some((720, 1604)),
        "the visible geometry after decoding"
    );
    assert_eq!(
        decoder.geometry().map(|geometry| geometry.coded),
        Some((720, 1616)),
        "the coded geometry after decoding"
    );

    for (index, frame) in frames.iter().enumerate() {
        let bgra = pixels(frame, &device);
        describe(index, frame, &bgra);
        check(index, frame.size(), &bgra);
    }
    eprintln!(
        "GPU decode: {} frames verified, device textures: {}",
        frames.len(),
        decoder.is_hardware()
    );
}
