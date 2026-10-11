//! Runtime test for the product's one-second statistics sample.
//!
//! The sampler lives inside the session thread (`video::play`), which owns the
//! decode threads and the mirror window, so this cannot be a unit test. Point
//! `HERMESGATE_H264_SAMPLE` at a raw Annex-B H.264 file and run:
//!
//! ```text
//! HERMESGATE_H264_SAMPLE=/path/raw.h264 cargo test --release --test video_stats_live -- --nocapture
//! ```
//!
//! Without the variable the test is a no-op, so the suite stays green on a
//! machine with no sample (same convention as `h264_offline`). The stream comes
//! from a loopback socket, exactly as the product reads one from the device —
//! no phone, and therefore nothing to unlock.
//!
//! What it pins, in a real session: the counters are published through
//! `video::stats()` while the session decodes, the numbers describe that run,
//! and the sample is retracted once the session ends, so a UI can never read a
//! finished session's numbers as live.
//!
//! It creates a mirror window for the duration (`RUN`), as the product does.

use hermesgate_lib::core::video::{self, PlaybackControl, StreamEnd};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long the fake device keeps the connection open after the stream.
const RUN: Duration = Duration::from_secs(3);
/// 1 Hz samples: two of them need more than two seconds.
const WAIT_FOR_SAMPLES: Duration = Duration::from_secs(12);
/// After the stream ends, how long the retraction may take.
const WAIT_FOR_RETRACTION: Duration = Duration::from_secs(8);

#[test]
fn a_running_session_publishes_counters_and_retracts_them_at_the_end() {
    let Ok(path) = std::env::var("HERMESGATE_H264_SAMPLE") else {
        eprintln!("HERMESGATE_H264_SAMPLE is unset: skipping the live statistics test");
        return;
    };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));

    let listener = TcpListener::bind("127.0.0.1:0").expect("binding a loopback socket");
    let address = listener.local_addr().expect("reading the bound address");

    // The fake device: deliver the whole stream, then hold the connection
    // open, the way a phone does while the screen changes nothing.
    let feeder = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accepting the session");
        socket.write_all(&bytes).expect("writing the stream");
        socket.flush().ok();
        std::thread::sleep(RUN);
    });

    let stream = Arc::new(TcpStream::connect(address).expect("connecting to the fake device"));
    let control = Arc::new(PlaybackControl::new());
    let (ended_tx, ended_rx) = mpsc::channel();
    let session = {
        let control = control.clone();
        std::thread::spawn(move || {
            let end = video::play(stream, control, "hermesgate stats test", |_, _| {});
            let _ = ended_tx.send(end);
        })
    };

    // Collect the samples a UI would have received, at the rate it polls.
    let deadline = Instant::now() + WAIT_FOR_SAMPLES;
    let mut samples = Vec::new();
    while Instant::now() < deadline && samples.len() < 2 {
        std::thread::sleep(Duration::from_millis(200));
        if let Some(sample) = video::stats() {
            if samples.last() != Some(&sample) {
                samples.push(sample);
            }
        }
    }

    assert!(
        !samples.is_empty(),
        "a running session published no sample within {WAIT_FOR_SAMPLES:?}"
    );
    let first = samples[0];
    eprintln!(
        "sample 1: fps {:.1}, {:.2} Mbit/s, decode {:.1} ms/s, present {:.1} ms/s, \
         overwritten {}, decoded {}",
        first.fps,
        first.bytes_per_s * 8.0 / 1e6,
        first.decode_ms,
        first.blit_ms,
        first.overwritten,
        first.frames_decoded
    );
    assert!(first.fps > 1.0, "decoded fps: {}", first.fps);
    assert!(first.fps < 1000.0, "decoded fps: {}", first.fps);
    assert!(
        first.bytes_per_s > 0.0,
        "the first window includes the delivered stream, so it must show \
         received bytes, got {}",
        first.bytes_per_s
    );
    assert!(first.decode_ms >= 0.0 && first.decode_ms.is_finite());
    assert!(first.blit_ms >= 0.0 && first.blit_ms.is_finite());
    assert!(first.frames_decoded >= 8, "{}", first.frames_decoded);
    // Frames the presenter never took cannot exceed the frames delivered.
    assert!(first.overwritten <= first.frames_decoded);
    for (index, sample) in samples.iter().enumerate().skip(1) {
        eprintln!(
            "sample {}: fps {:.1}, {:.2} Mbit/s, overwritten {}, decoded {}",
            index + 1,
            sample.fps,
            sample.bytes_per_s * 8.0 / 1e6,
            sample.overwritten,
            sample.frames_decoded
        );
    }

    // The stream ends, the session winds down, and the numbers stop being
    // published: a finished session must not keep feeding the UI.
    feeder.join().expect("the feeder thread");
    let end = ended_rx
        .recv_timeout(WAIT_FOR_RETRACTION)
        .expect("the session thread must return once the stream ends");
    eprintln!("session end: {end:?}");
    assert!(
        matches!(end, StreamEnd::Disconnected),
        "a stream that just stops is a disconnect, got {end:?}"
    );

    let deadline = Instant::now() + WAIT_FOR_RETRACTION;
    while Instant::now() < deadline && video::stats().is_some() {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        video::stats(),
        None,
        "the sample must be retracted when the session ends"
    );
    session.join().expect("the session thread");
}
