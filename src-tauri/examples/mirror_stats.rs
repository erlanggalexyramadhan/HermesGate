//! Measurement harness for P0 performance parity work.
//!
//! Not part of the app: it calls the **real** entry point the UI uses
//! (`mirroring::request_start` → adb push/forward → scrcpy server →
//! `video::play`), so the numbers describe the shipped pipeline rather than a
//! copy of it. Set `HERMESGATE_VIDEO_STATS=<csv>` to get the per-second
//! pipeline counters written by `video.rs`.
//!
//! Usage:
//!   cargo run --release --example mirror_stats -- [serial] [seconds]

use hermesgate_lib::core::mirroring::{self, MirrorQuality};
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let serial = args.next().unwrap_or_else(|| "bc058b2c".to_string());
    let seconds: u64 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(15);

    println!(
        "harness: serial={serial} seconds={seconds} stats={}",
        std::env::var("HERMESGATE_VIDEO_STATS").unwrap_or_else(|_| "(off)".to_string())
    );

    let started = Instant::now();
    if let Err(error) = mirroring::request_start(&serial, MirrorQuality::default()) {
        println!("harness: SESSION FAILED: {error}");
        std::process::exit(1);
    }
    println!("harness: start returned in {:?}", started.elapsed());

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut last = String::new();
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(1));
        let status = mirroring::poll_status();
        let line = format!("{}x{} {}", status.width, status.height, status.phase);
        if line != last {
            println!("harness: status {line} {}", status.reason);
            last = line;
        }
        // The same 1 Hz sample the UI reads, through the same channel.
        match status.stats {
            Some(stats) => println!(
                "harness: stats fps={:.1} mbit/s={:.1} decode_ms={:.1} blit_ms={:.1} \
                 overwritten={} decoded={}",
                stats.fps,
                stats.bytes_per_s * 8.0 / 1e6,
                stats.decode_ms,
                stats.blit_ms,
                stats.overwritten,
                stats.frames_decoded
            ),
            None => println!("harness: stats none"),
        }
    }

    let status = mirroring::poll_status();
    println!(
        "harness: final {}x{} running={} phase={} reason={} stats={}",
        status.width,
        status.height,
        status.running,
        status.phase,
        status.reason,
        if status.stats.is_some() {
            "present"
        } else {
            "none"
        }
    );
    let _ = mirroring::request_stop();
    let after = mirroring::poll_status();
    println!(
        "harness: session ended after {:?} stats_after_stop={}",
        started.elapsed(),
        if after.stats.is_some() {
            "still present (BUG)"
        } else {
            "retracted"
        }
    );
}
