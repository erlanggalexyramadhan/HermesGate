//! Audio engine: the device's own playback, played on the desktop.
//!
//! Scope: the session asks the scrcpy server for `audio=true` with
//! `audio_codec=raw`, so the audio socket carries plain PCM instead of
//! Opus/AAC/FLAC. scrcpy's `AudioConfig` fixes that format at 48 kHz, stereo,
//! 16-bit, and `raw_stream=true` removes the stream header and the per-packet
//! frame meta — so the socket is a byte stream that goes straight to the audio
//! device and nothing has to be decoded.
//!
//! Playback is `waveOut` (winmm, part of Windows): a small queue of prepared
//! buffers, refilled as the driver marks them done. Two consequences worth
//! stating: the queue is short (8 x 4096 bytes, ~0.17 s), so a *sustained*
//! byte rate proves the render device is consuming — if it stopped, the queue
//! would fill and reads would stall; and `played_ms` reports the device's own
//! clock, which is the direct evidence that audio reached the output.
//!
//! Failure policy: an audio device that cannot be opened, or a device that
//! refuses to capture playback, must never take the video session down. The
//! socket is drained either way and the reason is published for the UI.

use serde::Serialize;
use std::io::Read;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    waveOutClose, waveOutGetDevCapsW, waveOutGetID, waveOutGetPosition, waveOutOpen,
    waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader, waveOutWrite, CALLBACK_NULL,
    HWAVEOUT, WAVEFORMATEX, WAVEHDR, WAVEOUTCAPSW, WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_DONE,
};
use windows::Win32::Media::{MMTIME, TIME_BYTES};

/// scrcpy's `AudioConfig`: fixed at 48 kHz, stereo, 16-bit PCM.
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
pub const BITS_PER_SAMPLE: u16 = 16;

/// Bytes in one PCM frame (both channels, one sample each).
const BYTES_PER_FRAME: u32 = (CHANNELS as u32) * (BITS_PER_SAMPLE as u32 / 8);

/// One queued buffer: ~21 ms of audio, the same size class scrcpy reads in
/// (`AudioConfig.MAX_READ_SIZE`).
const CHUNK: usize = 4096;

/// Buffers kept queued ahead of the device. Short on purpose: it bounds how
/// far the audio can drift behind the picture, and its draining is what shows
/// the device is consuming.
const QUEUE: usize = 8;

/// What the UI can say about audio right now. Every field is a measurement:
/// `kbps` and `peak` are the last sample window, `played_ms` is the render
/// device's own clock, `error` is why there is no audio at all.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStats {
    /// Name of the render device the session opened (empty if it failed).
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// PCM bytes read off the socket in the last window, per second.
    pub kbps: f64,
    /// Highest absolute sample value in the last window, `0` = silence.
    pub peak: u16,
    /// Audio the render device has actually consumed, by its own clock.
    pub played_ms: u64,
    /// PCM bytes since the stream started.
    pub total_bytes: u64,
    /// Why audio is not playing (empty while it is).
    pub error: String,
}

static SNAPSHOT: Mutex<Option<AudioStats>> = Mutex::new(None);

fn publish(sample: Option<AudioStats>) {
    let mut guard = match SNAPSHOT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = sample;
}

/// The last published sample, or `None` when no audio stream is running.
pub fn stats() -> Option<AudioStats> {
    match SNAPSHOT.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Forget the previous stream's numbers before a session starts.
pub fn reset() {
    publish(None);
}

/// The render device, with a queue of prepared buffers around it.
struct Sink {
    handle: HWAVEOUT,
    headers: Vec<WAVEHDR>,
    buffers: Vec<Box<[u8]>>,
    free: Vec<bool>,
    device: String,
    /// The handle is closed once; `Drop` must not close it a second time.
    closed: bool,
}

/// Decode a render device's name from the raw bytes of a packed
/// `WAVEOUTCAPSW` (`szPname`, UTF-16, NUL-padded).
fn device_name(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[low, high]| u16::from_le_bytes([*low, *high]))
        .collect();
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

impl Sink {
    /// Open the default render device for 48 kHz stereo 16-bit PCM.
    fn open() -> Result<Self, String> {
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: CHANNELS,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * BYTES_PER_FRAME,
            nBlockAlign: BYTES_PER_FRAME as u16,
            wBitsPerSample: BITS_PER_SAMPLE,
            cbSize: 0,
        };
        let mut handle = HWAVEOUT::default();
        let result = unsafe {
            waveOutOpen(
                Some(&mut handle),
                WAVE_MAPPER,
                &format,
                None,
                None,
                CALLBACK_NULL,
            )
        };
        if result != 0 {
            return Err(format!(
                "no audio output device accepts {SAMPLE_RATE} Hz stereo 16-bit (MMSYSERR {result})"
            ));
        }

        // The device's own name, so the UI reports where the sound went.
        let mut id = 0u32;
        let mut caps = WAVEOUTCAPSW::default();
        let named = unsafe { waveOutGetID(handle, &mut id) } == 0
            && unsafe {
                waveOutGetDevCapsW(id as usize, &mut caps, size_of::<WAVEOUTCAPSW>() as u32)
            } == 0;
        let device = if named {
            // `WAVEOUTCAPSW` is packed, so its name field sits at alignment 1:
            // read it as bytes (alignment 1) and decode them here. Copying the
            // units as `u16` is undefined behaviour even when the pointer is
            // only read, and debug builds abort on it — which they did.
            let mut name = [0u8; 64];
            unsafe {
                std::ptr::copy_nonoverlapping(
                    std::ptr::addr_of!(caps.szPname) as *const u8,
                    name.as_mut_ptr(),
                    name.len(),
                )
            };
            device_name(&name)
        } else {
            String::new()
        };

        let mut sink = Self {
            handle,
            headers: Vec::with_capacity(QUEUE),
            buffers: Vec::with_capacity(QUEUE),
            free: vec![true; QUEUE],
            device,
            closed: false,
        };
        for _ in 0..QUEUE {
            // The buffers must not move: the prepared header holds their
            // address, so they live in boxed slices owned here.
            let mut buffer = vec![0u8; CHUNK].into_boxed_slice();
            let mut header = WAVEHDR {
                lpData: windows::core::PSTR(buffer.as_mut_ptr()),
                ..Default::default()
            };
            if unsafe {
                waveOutPrepareHeader(sink.handle, &mut header, size_of::<WAVEHDR>() as u32)
            } != 0
            {
                let error = "the audio device refused a playback buffer".to_string();
                sink.finish();
                return Err(error);
            }
            sink.buffers.push(buffer);
            sink.headers.push(header);
        }
        Ok(sink)
    }

    /// Mark every buffer the driver has finished with as reusable.
    fn reap(&mut self) {
        for index in 0..self.headers.len() {
            if !self.free[index] && (self.headers[index].dwFlags & WHDR_DONE) != 0 {
                self.headers[index].dwFlags &= !WHDR_DONE;
                self.free[index] = true;
            }
        }
    }

    /// Queue `data`, waiting for the device to free buffers as needed.
    fn play(&mut self, data: &[u8], stop: &AtomicBool) {
        let mut offset = 0;
        while offset < data.len() {
            self.reap();
            let slot = self.free.iter().position(|free| *free);
            let Some(index) = slot else {
                // Every buffer is still with the driver: this is the device
                // being slower than the stream, not an error.
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            let take = CHUNK.min(data.len() - offset);
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data[offset..].as_ptr(),
                    self.buffers[index].as_mut_ptr(),
                    take,
                );
            }
            let header = &mut self.headers[index];
            header.dwBufferLength = take as u32;
            header.dwFlags &= !WHDR_DONE;
            let written =
                unsafe { waveOutWrite(self.handle, header, size_of::<WAVEHDR>() as u32) } == 0;
            self.free[index] = !written;
            if !written {
                return; // the device is gone; keep draining the socket
            }
            offset += take;
        }
    }

    /// Bytes the render device has played out, converted to milliseconds.
    fn played_ms(&self) -> u64 {
        let mut time = MMTIME {
            wType: TIME_BYTES,
            ..Default::default()
        };
        if unsafe { waveOutGetPosition(self.handle, &mut time, size_of::<MMTIME>() as u32) } != 0 {
            return 0;
        }
        let bytes = unsafe { time.u.cb } as u64;
        bytes * 1000 / (SAMPLE_RATE as u64 * BYTES_PER_FRAME as u64)
    }

    /// Stop playback and release the device. Safe to call more than once.
    fn finish(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        unsafe {
            waveOutReset(self.handle);
            for header in self.headers.iter_mut() {
                let _ = waveOutUnprepareHeader(self.handle, header, size_of::<WAVEHDR>() as u32);
            }
            let _ = waveOutClose(self.handle);
        }
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.finish();
    }
}

fn sample(sink: Option<&Sink>, error: &str, bytes: u64, peak: u16, total: u64) -> AudioStats {
    AudioStats {
        device: sink.map(|sink| sink.device.clone()).unwrap_or_default(),
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        kbps: bytes as f64 * 8.0 / 1000.0,
        peak,
        played_ms: sink.map(Sink::played_ms).unwrap_or(0),
        total_bytes: total,
        error: error.to_string(),
    }
}

/// Read the audio socket into the render device until the stream ends, the
/// session stops, or the socket goes away.
///
/// The socket is read with a timeout so `stop` is honoured within ~200 ms:
/// the session's teardown must not wait on a device that has gone quiet.
pub fn play(stream: TcpStream, stop: Arc<AtomicBool>) {
    let mut stream = stream;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let (mut sink, error) = match Sink::open() {
        Ok(sink) => (Some(sink), String::new()),
        Err(reason) => (None, reason),
    };
    if !error.is_empty() {
        // Publish immediately: "no audio device" is the answer whether or not
        // the device ever sends bytes.
        publish(Some(sample(None, &error, 0, 0, 0)));
    }

    let mut buffer = vec![0u8; CHUNK];
    let mut window_bytes = 0u64;
    let mut total = 0u64;
    let mut peak = 0u16;
    let mut window = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                window_bytes += read as u64;
                total += read as u64;
                // Peak of the last window: silence reads 0, which is what
                // separates "forwarded silence" from "forwarded sound".
                for [low, high] in buffer[..read - read % 2].as_chunks::<2>().0 {
                    peak = peak.max(i16::from_le_bytes([*low, *high]).unsigned_abs());
                }
                if let Some(sink) = sink.as_mut() {
                    sink.play(&buffer[..read], stop.as_ref());
                }
            }
            Err(failure)
                if matches!(
                    failure.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
        if window.elapsed() >= Duration::from_secs(1) {
            publish(Some(sample(
                sink.as_ref(),
                &error,
                window_bytes,
                peak,
                total,
            )));
            window_bytes = 0;
            peak = 0;
            window = Instant::now();
        }
    }
    // The stream is gone: retract the counters rather than let the UI show a
    // finished stream's numbers as if they were live.
    publish(None);
    if let Some(mut sink) = sink.take() {
        sink.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_matches_the_server() {
        // scrcpy's AudioConfig: 48000 Hz, 2 channels, 16-bit -> 4 bytes a frame.
        assert_eq!(BYTES_PER_FRAME, 4);
        assert_eq!(SAMPLE_RATE * BYTES_PER_FRAME, 192_000);
        // One queued buffer is ~21 ms of audio, the class scrcpy reads in.
        assert!(CHUNK as u32 / BYTES_PER_FRAME == 1024);
    }

    #[test]
    fn the_device_name_is_decoded_from_the_packed_field() {
        let mut bytes = [0u8; 64];
        for (index, unit) in "Speakers (Realtek)".encode_utf16().enumerate() {
            bytes[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(device_name(&bytes), "Speakers (Realtek)");
        // An empty name field decodes to nothing, not to garbage.
        assert_eq!(device_name(&[0u8; 64]), "");
    }

    #[test]
    fn a_sample_reports_what_was_measured() {
        let stats = sample(None, "no device", 24_000, 12_000, 48_000);
        assert_eq!(stats.kbps, 192.0);
        assert_eq!(stats.peak, 12_000);
        assert_eq!(stats.total_bytes, 48_000);
        assert_eq!(stats.played_ms, 0);
        assert_eq!(stats.error, "no device");
    }

    #[test]
    fn stats_are_retracted_when_nothing_publishes_them() {
        publish(Some(sample(None, "", 0, 0, 0)));
        assert!(stats().is_some());
        reset();
        assert!(stats().is_none());
    }
}
