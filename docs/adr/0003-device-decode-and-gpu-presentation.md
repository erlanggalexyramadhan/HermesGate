# ADR 0003: Mirroring decodes on the device and presents on the GPU

- **Status:** Accepted
- **Date:** 2026-10-09
- **Supersedes:** the 2026-10-09 decision "mirroring decodes in software;
  device decoding is opt-in" (same file, earlier revision).
- **Updated:** 2026-10-11 - device decode and GPU presentation turned on by
  default; copy-ring geometry fixed on measured evidence.

## Context

`core/video.rs` decodes the mirroring stream one access unit at a time through
the Media Foundation H.264 decoder MFT (`core/h264.rs`) and shows the pictures
in the mirror window. Two presentation paths exist:

- **GDI**: the decoded picture is read back to system memory, converted to BGRA
  on the CPU and stretched onto the window with `StretchDIBits`.
- **GPU** (`core/gpu.rs`): a Direct3D 11 device, an `IMFDXGIDeviceManager`
  handed to the MFT, an `ID3D11VideoProcessor` for NV12 -> RGB and scaling, and
  a DXGI swap chain for presentation. Device decode and GPU presentation were
  both behind opt-in environment switches, defaulting to off.

The earlier revision of this ADR chose software decode as the shipping
pipeline because the device path had never run against live mirroring: the
`ProcessOutput` deadlock had only just been diagnosed as a caller-side message
ordering bug and fixed offline. That reasoning does not survive the
measurements below, and a live-only defect the offline tests could not reach
was found on the way.

## Root cause: the copy ring was built for the wrong surface size

The decoder announces its output size twice. Before it has parsed the stream it
offers a placeholder type - measured on this device, NV12 at **1920x1080** - and
after the first access unit it renegotiates the real size, **720x1616 coded**
with a **720x1604 visible aperture** (a 720-wide phone screen). The AVC stream
itself is not 1080p; the first offer is a placeholder, not a real renegotiation.

`h264.rs` built the `Copier` ring once, on the first negotiation:

```rust
if self.hardware && self.copier.is_none() { ... }
```

The ring was therefore four 1920x1080 NV12 surfaces while the decoder's own
surfaces were 720x1616. `CopySubresourceRegion` copied the 720x1616 source into
the top-left corner of each 1920x1080 ring surface, leaving the rest of the ring
holding whatever it was allocated with, and the presenter then read a source
rectangle of 720x1604 from a surface that only has 1080 rows of copied picture.

Measured trace (temporary `HERMESGATE_H264_DEBUG=1` switch, since removed; one
capture):

```
mirror: output type negotiated ((1920, 1080), hardware=true)   <- placeholder
mirror: output type negotiated ((720, 1616), hardware=true)    <- real
mirror: copy: decoded 720x1616 | ring 1920x1080 | geometry coded=(720, 1616) visible=(720, 1604)
vp: blit coded=(720, 1616) visible=(720, 1604)
         source_rect=RECT { 0, 0, 720, 1604 }   <- 1604 rows from a 1080-row surface
         input_texture=(1920x1080)
```

The visible symptom matches the arithmetic exactly. `1080 / 1604 = 67.3%`, and
the captured window was garbage from row **618 of 918** (67.3%) downward: 300
green rows, row-mean PSNR 49.5 dB above the boundary and 11.5 dB below it.

## Fix

1. **Rebuild the ring when the negotiated size changes** (`h264.rs`): the
   copier is rebuilt whenever the negotiated dimensions differ from the ring's
   own, instead of only when absent.
2. **Clamp every copy and source rectangle to the surface it reads**
   (`gpu.rs`): `overlap()` takes the smaller extent of a pair of surfaces,
   rounded down to a whole chroma pair, with a one-pixel floor; `Copier::copy`
   copies that region
   explicitly and `Converter::convert` takes its stream source rectangle from
   it. A size mismatch can no longer read past the end of either surface.

The coded-versus-visible split stays: the video processor enumerator is built
for the coded surface, the stream source rectangle is the visible aperture.

## Decision

- **Device decode and GPU presentation are the shipping default.** Opt out with
  `HERMESGATE_H264_HW=0` (software decode) and `HERMESGATE_VIDEO_GPU=0` (GDI
  presentation); both are supported and still tested.
- Software decode plus the GDI presenter remains the fallback for machines with
  no usable Direct3D adapter, and the GDI presenter keeps
  `SetStretchBltMode(HALFTONE)` + `SetBrushOrgEx` so its downscale is not the
  aliased `COLORONCOLOR` one.
- The message order that fixed the deadlock is a contract, not a convention:
  `SET_D3D_MANAGER` -> `SetInputType` -> `BEGIN_STREAMING`/`START_OF_STREAM` ->
  first `ProcessInput`.

## Evidence

Live device `bc058b2c`, Settings list scrolled for 25 s per run, 3 runs per
configuration, identical workload, `tail_fps` measured over the last third of
each run:

| Configuration | tail FPS | CPU-s / 25 s | read % | decode % | decode ms | blit ms | paint ms |
|---|---|---|---|---|---|---|---|
| A pre-fix, software decode + GDI | 49.5 | 15.5 | 58 | 41 | 422.0 | 71.5 | 66.5 |
| B software decode + GDI HALFTONE | 51.3 | 20.3 | 57 | 43 | 438.8 | 230.4 | 225.3 |
| C pre-fix, device decode + GPU | 51.1 | 8.7 | 98 | 2 | 19.6 | 464.0 | 214.2 |
| D pre-fix, software decode + GPU | 48.5 | 15.2 | 61 | 38 | 395.8 | 32.4 | 0.0 |
| **E fixed, device decode + GPU (shipped)** | **51.1** | **1.2** | **99** | **1** | **17.0** | **10.0** | **0.0** |
| scrcpy v4.1 reference client | 49.9 | 4.8 | - | - | - | - | - |

Quality, static Settings scene, PSNR against the device screenshot scaled to the
window with lanczos (the ideal resample):

| Capture | PSNR vs same ideal |
|---|---|
| pre-fix GDI, COLORONCOLOR | 23.8 dB |
| GDI, HALFTONE + SetBrushOrgEx | 29.5 dB |
| pre-fix device decode + GPU (green block) | 16.2 dB |
| **fixed device decode + GPU** | **30.6 dB** |
| fixed GDI (software decode) | 29.8 dB |

Against each capture's own device screenshot the fixed GPU path scores 35.0 dB
and the fixed GDI path 34.9 dB; both show zero garbage rows and content to the
last row (the Android navigation pill is visible at the bottom edge).

Pair validation done again on 2026-10-11 (`tools/bench/verify_capture.sh`)
compares the mirrored frame with the device screenshot row by row at the same
size. A crop or a shift shows up as a best vertical alignment other than zero:
the fixed GPU capture aligns at shift **0** rows, and shifting by one row costs
about six times more error. Dark rows have to be the *device's* dark rows and not
a band of their own: that capture has 211 near-black rows against the device
screenshot's own 213, at the same positions. Two independent post-fix captures of
the static scene agree at 38.2 dB, and a fresh capture at 06:44 agrees with its
own device screenshot at 39.0 dB with zero garbage rows.

Runs made while the peer display was asleep reported *software* decode in the
status line. That report was wrong, and there was no fallback. The banner printed
on the first decode call, which carries no complete access unit yet, so it named
the initial value before the output type — the thing that decides the path — had
been negotiated. A diagnostic build (`HERMESGATE_H264_DEBUG=1`) shows one run
printing "decoding on the CPU" *between* "accepted the Direct3D 11 device
manager" and "output type negotiated ((1920, 1080), hardware=true)", and a
device-asleep run printing "decoding on the GPU" after its first negotiation,
with identical negotiation inputs in both (no cooldown, device accepted, hardware
adapter, NV12 offered, samples provided). The banner now waits for the first
decoded frame, which cannot exist before the output type is set.

The harness that produced these numbers lives in `tools/bench/`: `bench.sh`
(FPS/CPU, one configuration per invocation, counters via
`HERMESGATE_VIDEO_STATS`), `kwal.sh` (quality capture: device screenshot plus
mirror-window capture, compared against the screenshot scaled with lanczos),
`cap.ps1` (window capture validated with `GetWindowRect`), `scrcpy_ref.sh`
(the reference client on the identical workload, with a graceful shutdown) and
`verify_capture.sh` (one live capture plus the row-alignment pair check
described above). A PSNR
from a capture pair is only meaningful when both shots hold the same pixels:
one run of this campaign scored 17.3 dB on a frame with zero defects because a
heads-up notification appeared on the device between the two shots. Validate
the pair (no garbage rows, frame checked) before quoting the number.

## The software path declared the wrong colour range (fixed 2026-10-11)

The software decode path hands the GPU a BGRA frame that is already
display-referred full range, but `rgb_color_space()` declared
`RGB_Range = 1` / `Nominal_Range = 0`. In `d3d11.h`,
`D3D11_VIDEO_PROCESSOR_COLOR_SPACE` uses **0 = full, 1 = limited**, and
`D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE` uses **2 = 0-255, 0 = undefined** (`0` is
`..._UNDEFINED`, not 0-255). The declaration therefore told the video processor
to treat full-range pixels as studio range and stretch them back out.

Measured on one identical scene (mirror vs. the matching device screenshot,
`tools/bench/scatter.py`):

| configuration | gain | intercept | PSNR |
|---|---|---|---|
| software + GPU, before | 1.13 | -14 | 28.94 dB |
| software + GPU, after (FIXSW2 / FIXSW3) | 0.98 / 0.98 | +2.7 / +2.6 | 33.41 / 33.38 dB |
| software + GPU, after (same-scene pair `SAMEB`) | 0.98 | +2.7 | 33.41 dB |
| device decode + GPU, same scene (`SAMEA`) | 0.99 | +1.9 | 36.19 dB |
| device decode + GPU, other bright-reference runs | 0.98 | +2.0 | 39.4-39.5 dB |

The pre-fix numbers are what a 255/219 expansion of the post-fix content would
give (0.98/+2.7 -> 1.141/-15.5 against a measured 1.13/-14, within 1%), which is
why the mis-declaration is the cause and not a coincidence.

Fix: `RGB_FULL_RANGE` + `NOMINAL_RANGE_0_255` in `gpu.rs`. The NV12 path
(`yuv_color_space()`) is untouched, so the shipping device-decode default cannot
be affected; re-verified anyway on the rebuilt binary (36.15 dB, valid pair:
dev1-vs-dev2 = 38.0 dB, mirror mean 82/80/84 vs device 81/79/82).

Not explained: the software path still trails the device path on the same scene,
by **2.8 dB** on a same-scene A/B pair (`SAMEA` device vs `SAMEB` software, one
shared reference, pair control `dev1~dev2` = 38.5 dB): 33.41 dB vs 36.19 dB, rmse
6.3 vs 4.6. An earlier ~6 dB figure compared references shot in different runs,
which differ by 25-30 dB and are therefore not comparable. The residual is
per-pixel rather than geometric (shifting the reference costs 11.8-18.7, 3x3
blurring costs 8.5), largest on dark content (rmse 8.4 at saturation < 10 vs
5.0-5.3 elsewhere) and finer than the device path's own residual (within-2x2
variance ratio 0.54 vs 0.31-0.43), which is consistent with the software path's
own NV12->BGRA conversion (`src/core/pixels.rs`: nearest 2x2 chroma, BT.601
studio matrix, integer rounding) versus the GPU video processor's conversion -
consistent with, not proof of. No code change: the software path is the opt-out.

## Consequences

- The decoder no longer sets the FPS ceiling; the pipeline is paced by the
  display (vsync present) at the phone's stream rate. CPU cost is now the
  lowest of every configuration measured, including scrcpy's reference client.
- `blit_ms` is a correctness signal as much as a performance one: 464 ms with a
  ring the size of the placeholder, 10 ms with the ring the size of the picture.
  A future size mismatch will show up there.
- Live hardware decode is no longer "unproven": the device path has run against
  a real device across repeated 25 s and 60 s sessions, including captures and
  benchmarks, with `hardware=true` and D3D11 texture samples.
  `tests/h264_offline.rs::decodes_sample_on_the_gpu` stays `#[ignore]`d as a
  bounded `--ignored` regression check, because it still hangs `cargo test` if
  the message ordering regresses.
- Correction to an earlier version of this ADR: `windows` 0.62.2 *does* expose
  `MFCreateVideoSampleAllocatorEx` (as well as `MFCreateDXGISurfaceBuffer`), so
  the Media Session / renderer-owned allocator route needs no hand-written
  declaration.

## Not covered

- Wireless transport performance under sustained load — **measured 2026-10-11**: tail FPS 50.5 (Wi-Fi) vs 51.1 (USB), CPU 1.05-1.30 vs 1.19-1.30 CPU-s/25 s, ~8.5 Mbit/s on both, ping 0% loss, so the transport is equivalent to USB for this workload. The one stalled wireless session was a frame-flow stall (a dark/locked device sends no frames), reproduced with `tools/bench/stall-repro.sh`, not a transport failure.
- The 2.8 dB residual of the software-decode path (previous section), and the run-to-run variability of PSNR references: two device screenshots of the same nominal screen from different runs differ by 25-30 dB, so a path must be compared against its own run's reference (device path: 36.19 dB on a same-scene pair, 39.4-39.5 dB against other bright references, 36.15 dB on the dark Settings screen).
- The statistics surface scrcpy exposes (dropped frames, end-to-end latency per
  frame, encoder metrics) is not implemented; the numbers above come from the
  measurement harness, not from the product.
- `HERMESGATE_VIDEO_STATS` remains the measurement switch for the harness; it
  costs nothing when unset.
