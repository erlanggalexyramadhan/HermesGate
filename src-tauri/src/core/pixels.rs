//! Pixel layout conversions shared by the decode and render stages.
//!
//! Both the software decoder (system-memory frames) and the GPU fallback
//! readback path (`gpu::Device::read_bgra`) need to turn a decoded NV12
//! picture into tightly packed BGRA rows, so the conversion lives here once.
//! Nothing in this module touches the operating system: it is pure byte
//! arithmetic and is unit-tested directly.

/// Convert one NV12 frame to BGRA.
///
/// NV12 is planar: `coded_height` luma rows at `pitch`, then the
/// interleaved chroma plane, one chroma row per two luma rows. `pitch` may
/// exceed the visible width and `coded_height` the visible height (both are
/// coded padding), so only the visible window is converted. Colours use the
/// BT.601 studio-swing coefficients the stream is encoded with, in integer
/// arithmetic.
pub fn nv12_to_bgra(
    bytes: &[u8],
    pitch: usize,
    coded_height: u32,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("the frame geometry is empty".to_string());
    }
    let (width, height) = (width as usize, height as usize);
    let luma_end = (height - 1) * pitch + width;
    let chroma = pitch * coded_height as usize;
    let chroma_end = chroma + ((height - 1) / 2) * pitch + width;
    if bytes.len() < luma_end || bytes.len() < chroma_end {
        return Err(format!(
            "decoded NV12 frame is short: {width}x{height} visible at pitch {pitch} in a {coded_height}-row plane needs {chroma_end} bytes, has {}",
            bytes.len()
        ));
    }

    let mut bgra = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let luma_row = row * pitch;
        let chroma_row = chroma + (row / 2) * pitch;
        for column in 0..width {
            let luma = (bytes[luma_row + column] as i32 - 16) * 298;
            let pair = chroma_row + (column / 2) * 2;
            let u = bytes[pair] as i32 - 128;
            let v = bytes[pair + 1] as i32 - 128;
            let red = ((luma + 409 * v + 128) >> 8).clamp(0, 255) as u8;
            let green = ((luma - 100 * u - 208 * v + 128) >> 8).clamp(0, 255) as u8;
            let blue = ((luma + 516 * u + 128) >> 8).clamp(0, 255) as u8;
            bgra.extend_from_slice(&[blue, green, red, 255]);
        }
    }
    Ok(bgra)
}

/// Copy the visible rows of a frame the decoder already writes as BGRA
/// (`MFVideoFormat_RGB32`), which needs no colour conversion.
pub fn copy_rows(bytes: &[u8], pitch: usize, width: u32, height: u32) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("the frame geometry is empty".to_string());
    }
    let row_bytes = width as usize * 4;
    let end = (height as usize - 1) * pitch + row_bytes;
    if pitch < row_bytes || bytes.len() < end {
        return Err(format!(
            "decoded frame is short: {width}x{height} at pitch {pitch} needs {end} bytes, has {}",
            bytes.len()
        ));
    }
    let mut bgra = Vec::with_capacity(row_bytes * height as usize);
    for row in 0..height as usize {
        let start = row * pitch;
        bgra.extend_from_slice(&bytes[start..start + row_bytes]);
    }
    Ok(bgra)
}

#[cfg(test)]
mod tests {
    use super::{copy_rows, nv12_to_bgra};

    /// A 4x4 NV12 frame: luma 100 everywhere, neutral chroma, so every pixel
    /// must come out grey. The pitch carries two bytes of padding per row, so
    /// a conversion that ignores it reads the wrong bytes.
    #[test]
    fn converts_padded_nv12_to_opaque_bgra() {
        let pitch = 6usize;
        let coded_height = 4u32;
        let mut bytes = vec![100u8; pitch * coded_height as usize + pitch * 2];
        for row in 0..4usize {
            for column in 4..pitch {
                bytes[row * pitch + column] = 7;
            }
        }
        for byte in bytes.iter_mut().skip(pitch * coded_height as usize) {
            *byte = 128;
        }

        let bgra = nv12_to_bgra(&bytes, pitch, coded_height, 4, 4).expect("converting");
        assert_eq!(bgra.len(), 4 * 4 * 4);
        for pixel in bgra.chunks(4) {
            assert_eq!(pixel[3], 255, "alpha");
            // Neutral chroma and luma 100 land between the blue and grey
            // ramps; the point here is that all four pixels agree and none of
            // the padding leaked in.
            assert_eq!(pixel[0], pixel[1], "blue and green agree");
            assert_eq!(pixel[1], pixel[2], "green and red agree");
            assert!(pixel[0] > 90 && pixel[0] < 110, "grey level: {}", pixel[0]);
        }
    }

    #[test]
    fn rejects_a_short_nv12_frame() {
        let error = nv12_to_bgra(&[0u8; 16], 8, 4, 8, 4).expect_err("short frame");
        assert!(error.contains("short"), "unexpected message: {error}");
    }

    /// Four visible bytes per row inside a twenty-byte stride, so a copy that
    /// ignores the pitch drags padding into the second row.
    #[test]
    fn copies_rows_out_of_a_padded_rgb32_frame() {
        let pitch = 20usize;
        let height = 2usize;
        let mut bytes = vec![0u8; pitch * height];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        let bgra = copy_rows(&bytes, pitch, 4, height as u32).expect("copying");
        assert_eq!(bgra.len(), 16 * height);
        assert_eq!(&bgra[..16], &bytes[..16]);
        assert_eq!(&bgra[16..32], &bytes[pitch..pitch + 16]);
    }

    #[test]
    fn rejects_a_short_rgb32_frame() {
        let error = copy_rows(&[0u8; 4], 16, 4, 2).expect_err("short frame");
        assert!(error.contains("short"), "unexpected message: {error}");
    }
}
