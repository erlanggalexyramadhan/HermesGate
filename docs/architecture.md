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
  `adb devices -l`, maps each line to a state (`device` → online,
  `unauthorized` → unauthorized, anything else → offline), and marks devices no
  longer listed as disconnected. Online devices are identified once with
  `getprop` (manufacturer, model, Android version) and cached.
- **Interface:** the `list_devices` command returns a snapshot
  (devices + ADB status); a background watcher polls every 2 s and emits
  `devices-changed` only when the snapshot actually changes. The Devices view
  (`src/App.tsx`) reads both through the typed wrappers in `src/lib/tauri.ts`.
- **Transport:** each device is tagged `usb` or `wifi` from its serial
  (`ip:port` ⇒ Wi-Fi), so wireless devices will flow through the same
  `Transport` seam once pairing lands.

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
                          access unit per push; software by default
  presentation            core/video.rs — GDI presenter by default, Direct3D 11
                          swap chain when HERMESGATE_VIDEO_GPU=1
  HermesGate window(s)
```

The four environment switches, all off by default:

- `HERMESGATE_VIDEO_STATS=<csv>` — per-second pipeline counters (frames decoded,
  delivered, painted, overwritten, and the time spent in each stage).
- `HERMESGATE_VIDEO_GPU=1` — build the Direct3D 11 device path.
- `HERMESGATE_H264_HW=1` — decode on that device instead of in system memory.
- `HERMESGATE_H264_SAMPLE=<file>` — with `cargo test --test h264_offline`, decode
  a recorded Annex-B stream offline.

Device decoding is implemented but blocked: the Media Foundation decoder never
returns from its first `ProcessOutput` once a Direct3D 11 device manager is set,
which is why the default is software decoding. See
[ADR 0003](adr/0003-software-decode-by-default.md).

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
