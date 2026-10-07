# Roadmap

Phases are ordered by dependency. **Phase 0 and USB device detection (the first
part of Phase 1) are what this repository ships today**; everything else is
planned work and is not implemented yet.

## Phase 0 — Foundation *(done)*

- Tauri 2 application with a React + TypeScript frontend.
- Rust native core with module boundaries and a command layer.
- Connection layer shaped so USB and Wi-Fi share one pipeline (`Transport` seam).
- Light professional UI shell with dashboard and module registry.
- CI, documentation, ADRs, project metadata.

## Phase 1 — Device and connection *(current)*

Shipped:

- ADB integration: HermesGate bundles its own `adb` runtime (no Android SDK
  install), resolves it (override → bundled → `PATH` → SDK) and parses
  `adb devices -l` / `getprop` for state and metadata.
- Device Manager state exposed through the `list_devices` command and the
  `devices-changed` event, rendered in the Devices view.

Remaining:

- USB transport: connected-device sessions over the local ADB daemon.
- Wi-Fi transport: wireless pairing and discovery (mDNS / `adb pair`).

## Phase 2 — Mirroring pipeline

- Mirroring session established from an existing connection session
  (transport-agnostic by construction).
- Native decode and GPU rendering of the device screen.
- Main preview rendered in the dashboard window.
- Reuse proven capture/encode components on the Android side instead of
  writing a protocol from scratch.
- Frame data stays in native code; the webview only receives state.

## Phase 3 — Input

- Pointer/touch, keyboard and gesture injection over the active session.
- Input intent from the UI through commands/events, never per-frame traffic
  through JavaScript.

## Phase 4 — Clean mirror window (OBS)

- Borderless, resizable second window showing only the Android screen.
- Optional always-on-top; continues mirroring while the main window is
  minimized.
- Shares the single decoded/rendered output with the main preview (one decode,
  two presentations).

## Phase 5 — Clipboard, settings, packaging

- Clipboard synchronisation between desktop and device.
- Persisted settings (transport preference, mirror window behaviour).
- Release packaging and update channel.
