# ADR 0003: Mirroring decodes in software; device decoding is opt-in

- **Status:** Accepted
- **Date:** 2026-10-09
- **Updated:** 2026-10-09 - root cause found and fixed; findings revised.

## Context

`core/video.rs` decodes the mirroring stream one access unit at a time through
the Media Foundation H.264 decoder MFT (`core/h264.rs`), converts NV12 to BGRA
and paints the mirror window. Measured on this machine the pipeline runs at
~9-10 FPS with the decoder taking ~100% of one core and the presenter costing
almost nothing, so the decoder is the bottleneck and hardware decoding is the
only lever that moves it.

`core/gpu.rs` implements that lever: a Direct3D 11 device, an
`IMFDXGIDeviceManager` handed to the MFT with `MFT_MESSAGE_SET_D3D_MANAGER`, an
`ID3D11VideoProcessor` for NV12 -> BGRA and scaling, and a DXGI swap chain for
presentation.

## Findings

### Hardware MFT enumeration returns zero on this machine - by design, not by flags

A standalone probe (`scratch/mft_probe`, full matrix in its `REPORT.md`) ran
`MFTEnumEx` over every relevant flag/filter combination against SDK
10.0.26100.0. The flags are not too strict: with `MFT_ENUM_FLAG_HARDWARE` the
video-decoder category holds exactly two AMD MFTs, and neither declares an
H.264 input subtype - "AMD MFT MJPEG Decoder" (MJPG only) and "AMD D3D11
Hardware MFT Playback Decoder" (MP4S/M4S2/MP4V/DIVX/DX50/XVID only). The only
H.264 decoder registered on the box is the in-box "Microsoft H264 Video
Decoder MFT" (`CLSID_MSH264DecoderMFT`, `MFTFlags=0x1` SYNCMFT), which
`MFT_ENUM_FLAG_HARDWARE` excludes by definition. The AMD driver package
(`u0203303.inf`) registers no H.264 decoder MFT at all. So the vendor-MFT
selection in `activate_hardware_mft` (two-pass enum, with and without the
H.264 input filter) correctly finds nothing here and falls back to the in-box
decoder; hardware H.264 decode on this RX 6750 XT goes through the in-box
decoder's D3D11/DXVA path, not a vendor MFT.

### The ProcessOutput deadlock was a caller-side message-ordering bug - fixed

The MFT accepted the device (`MF_SA_D3D11_AWARE`), accepted the device-backed
NV12 output type, then never returned from the first
`IMFTransform::ProcessOutput`, blocking inside the decoder's DXVA frame
manager until the process was killed. It survived every variation tried
(sample-count attributes, caller-supplied `MFCreateDXGISurfaceBuffer` samples,
re-sent manager, multithread protection, CODECAPI hints, even a WARP device) -
but none of those variations changed message *ordering*, and a header audit of
SDK 10.0.26100.0 found exactly one ordering rule our sequence broke:

- `Mftransform.idl`: `MFT_MESSAGE_NOTIFY_START_OF_STREAM` is "send by pipeline
  **before processing the first sample**". We sent it (with `BEGIN_STREAMING`)
  only after the output type was set, i.e. after the first `ProcessInput` had
  already run.
- `SET_D3D_MANAGER` was sent after `SetInputType`; the documented convention
  is the device manager before the types and before streaming starts.

Fix (`ensure_media_types`): `bind_device()` (`SET_D3D_MANAGER`) runs first,
then `SetInputType`, then both streaming notifications, then the first
`ProcessInput`; the notifications are no longer re-sent on output
renegotiation. With that order the first `ProcessOutput` completes.

Verified offline against the recorded sample: `decodes_sample_on_the_gpu`
passes 4/4 runs (~2.5 s), 10 frames, geometry 720x1604, output samples are
D3D11 device textures (`device textures: true`), backend reported as hardware.

The SDK-side caveat stands: nothing in the headers documents the in-box
decoder as D3D11-capable - `MF_SA_D3D11_AWARE` is an undecorated GUID and only
`MF_SA_D3D_AWARE` carries documented "accepts a device manager" semantics. The
fix satisfies the one rule that is documented; live stability is unproven
(DEVICE-BLOCKED).

## Decision

- The shipping pipeline is the software decoder plus the GDI presenter: both
  are verified by `tests/h264_offline.rs` against a recorded stream and are
  the configuration the FPS numbers were measured on.
- Device decoding stays behind `HERMESGATE_H264_HW=1` (the D3D11 presentation
  pipeline additionally behind `HERMESGATE_VIDEO_GPU=1`), defaulting to off,
  until it has been validated on a real device - the offline fix has not run
  against live mirroring (rotation/`MF_E_TRANSFORM_STREAM_CHANGE`, sustained
  load are untested).
- `tests/h264_offline.rs::decodes_sample_on_the_gpu` stays `#[ignore]`d so a
  future ordering regression fails fast under a bounded `--ignored` run rather
  than hanging `cargo test`.

## Consequences

- Mirroring performance is what it was: the decoder, not presentation, sets
  the FPS ceiling; flipping the opt-in gate on is the next step once a device
  session can validate it.
- The decoder must keep the documented message order: `SET_D3D_MANAGER` ->
  `SetInputType` -> `BEGIN_STREAMING`/`START_OF_STREAM` -> first `ProcessInput`.
  Any future change there gets run under
  `cargo test --test h264_offline -- --ignored` with a timeout.
- Correction to an earlier version of this ADR: `windows` 0.62.2 *does* expose
  `MFCreateVideoSampleAllocatorEx` (as well as `MFCreateDXGISurfaceBuffer`), so
  the Media Session / renderer-owned allocator route needs no hand-written
  declaration.
