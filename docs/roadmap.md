# Roadmap

Phases are ordered by dependency. **Phase 0 is what this repository ships
today**; every later phase is planned work and is not implemented yet.

## Phase 0 — Foundation *(current)*

- Tauri 2 application with a React + TypeScript frontend.
- Rust native core with module boundaries and a command layer.
- Connection layer shaped so USB and Wi-Fi share one pipeline (`Transport` seam).
- Light professional UI shell with dashboard and module registry.
- CI, documentation, ADRs, project metadata.

## Phase 1 — Device and connection

- ADB integration: locate/spawn `adb`, list devices, watch attach/detach.
- USB transport: connected-device sessions over the local ADB daemon.
- Wi-Fi transport: wireless pairing and discovery (mDNS / `adb pair`).
- Device Manager state exposed through commands and rendered in the Devices view.

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
