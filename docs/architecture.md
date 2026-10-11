# Architecture

This document describes the **implemented** foundation and the constraints the
future mirroring implementation must respect. Items marked *planned* are design
direction, not existing behaviour.

## Layering

```
UI (React + TypeScript, webview)
  └── Tauri commands / events  — typed, capability-scoped, the only bridge
        └── Rust native core   — all privileged work happens here
              ├── device        Device Manager
              ├── connection    Connection Manager (usb / wifi)
              ├── adb           ADB integration
              ├── mirroring     Mirroring engine
              ├── input         Input engine
              ├── video         Decode + GPU render
              ├── clipboard     Clipboard sync
              └── settings      Persisted settings
```

Implemented today: the layering itself, the command layer
(`src-tauri/src/commands.rs`), the module tree
(`src-tauri/src/core/`), the module registry that the dashboard displays, and
device detection: `adb` locates and runs the bundled ADB runtime, `device` keeps
device state and exposes it through the `list_devices` command and the
`devices-changed` event. The `mirroring`, `input`, `video`, `clipboard` and
`settings` modules are *planned*.

## Security boundary

- The webview runs with the `core:default` capability only
  (`src-tauri/capabilities/default.json`); no plugin permissions are granted.
- No shell, filesystem, or device capability is exposed to JavaScript. The only
  frontend entry points are the Tauri commands registered in
  `src-tauri/src/lib.rs`, each implemented in Rust.
- A strict CSP is configured for release builds (`app.security.csp` in
  `src-tauri/tauri.conf.json`), with a dev-only relaxation for Vite HMR.

## Device detection (*implemented*)

- **ADB runtime:** HermesGate ships the official `adb` binary in
  `src-tauri/binaries/` (Windows; bundled as a resource via
  `tauri.conf.json → bundle.resources`) so ordinary users need no Android SDK.
  `src-tauri/src/core/adb.rs` resolves it in order: `HERMESGATE_ADB` override →
  bundled binary → `PATH` → default SDK install. It is the only module that
  spawns `adb`; calls time out after 10 s instead of hanging a command.
- **Discovery:** `core::device::DeviceManager::refresh()` runs
  `adb devices -l` and maps each line to a state (`device` → online,
  `unauthorized`/`authorizing` → unauthorized, anything else → offline). The
  picture is rebuilt from every refresh: a device that is not listed any more —
  or that is listed only on a transport that just went away — is simply not
  reported, so no stale "disconnected" card can linger.
- **Identity:** a device is identified once with `getprop`
  (`ro.serialno`, manufacturer, model, Android version) and grouped by that
  identity, because `adb` lists one phone several times over. On the bench
  (`bc058b2c`) it appears as its hardware serial, as the mDNS alias of the same
  USB link (`adb-bc058b2c-8STQEK._adb-tls-connect._tcp`) and as
  `192.168.18.7:39583` over wireless. One phone, one card; a device without
  `getprop` yet still groups by the serial embedded in an mDNS alias.
- **Interface:** the `list_devices` command returns a snapshot
  (devices + ADB status); a background watcher polls every 2 s and emits
  `devices-changed` only when the snapshot actually changes. The Devices view
  (`src/App.tsx`) reads both through the typed wrappers in `src/lib/tauri.ts`.
- **Transport:** each card carries a `DeviceEndpoint { transport, serial }` per
  live transport, USB first, each addressed by the most stable serial that
  transport has (hardware serial over its mDNS alias, `ip:port` over Wi-Fi). The
  Device page lets the user choose the endpoint, and mirroring is started with
  *that* serial, so a session runs over the transport the user picked rather
  than an arbitrary alias of the same phone. A transport that disappears takes
  its endpoint with it; the card survives as long as one transport is left.
- **Mirror window:** created as `WS_OVERLAPPEDWINDOW` sized with
  `AdjustWindowRectEx`, so the client area keeps the exact frame size from the
  device and Windows draws the caption, borders and close box and handles
  dragging, resizing and closing. No custom chrome and no overlay: the picture
  is the whole client area (see `tools/bench/hg-window.ps1`).

## Connection Manager: USB and Wi-Fi share one pipeline

`src-tauri/src/core/connection/` is the only layer that knows *how* a device is
reached. It exposes a `Transport` enum (`Usb` / `Wifi`); everything downstream —
mirroring, input, clipboard — consumes an opaque connection session instead of a
transport.

*Planned* consequence: USB and Wi-Fi differ only in how the session is
established (attached device over the local ADB daemon vs. wireless
pairing/discovery). Once a session exists, the mirroring and input pipelines are
identical for both transports, so no transport-specific code exists above the
connection layer.

## Mirroring pipeline

```
Android
  capture / encode        scrcpy server, device side
  transport               USB (ADB reverse tunnel) or Wi-Fi — connection layer only
  native decode           core/h264.rs — Media Foundation H.264 decoder MFT, one
                          access unit per push; device decode by default,
                          software decode as the fallback
  presentation            core/video.rs — Direct3D 11 video processor and swap
                          chain by default, GDI presenter as the fallback
  HermesGate window(s)
```

The four environment switches (device decode and GPU presentation are on unless
switched off):

- `HERMESGATE_VIDEO_STATS=<csv>` — per-second pipeline counters (frames decoded,
  delivered, painted, overwritten, and the time spent in each stage). Off by
  default; costs nothing when unset.
- `HERMESGATE_VIDEO_GPU=0` — draw with GDI instead of building the Direct3D 11
  device path.
- `HERMESGATE_H264_HW=0` — decode in system memory instead of on the device.
- `HERMESGATE_H264_SAMPLE=<file>` — with `cargo test --test h264_offline`, decode
  a recorded Annex-B stream offline. Unset by default.

Device decoding is the shipping default: the Media Foundation decoder returns
device textures once the device manager is set before the types and before
streaming starts, and the D3D11 video processor presents them. Software decode
and the GDI presenter remain the fallback (`HERMESGATE_H264_HW=0`,
`HERMESGATE_VIDEO_GPU=0`). See
[ADR 0003](adr/0003-device-decode-and-gpu-presentation.md).

Requirements this layout must keep:

1. **No frame data through JavaScript.** Encoded frames and decoded surfaces
   stay in native code; the webview receives state/controls only (commands and
   events), never pixels.
2. **Single decode, multiple presentations.** The decode/render path produces one
   output that can be presented by more than one window, so the main preview and
   a clean mirror window do not each decode the stream.
3. **Transport-agnostic.** The pipeline depends on a session, not on USB or
   Wi-Fi (see above).

## Clean mirror window for OBS (*planned*)

A second, separate Tauri window that:

- renders only the Android screen — no dashboard, sidebar or toolbar;
- is borderless and resizable;
- can stay always-on-top on request;
- keeps mirroring while the main window is minimized (the pipeline is owned by
   the Rust core, not by the main window's lifecycle).

Because presentation is shared (requirement 2), this window attaches to the
existing decoded output instead of starting a second pipeline. Implementation is
explicitly out of scope for the bootstrap; see the [roadmap](roadmap.md).

## Frontend structure

- `src/lib/tauri.ts` — typed wrappers around each Tauri command; the only place
  that calls `invoke`.
- `src/App.tsx` — application shell: sidebar navigation (mirroring and settings
  are visibly disabled), the dashboard, and the Devices view backed by the
  `list_devices` command / `devices-changed` event.
- `src/App.css` — design tokens and components; light, professional styling with
  no framework dependency.
