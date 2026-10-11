# Roadmap

Phases are ordered by dependency. **Phases 0-2 and the first slice of Phase 3 are
what v0.1.0 ships**; the rest is planned work and is not implemented yet.

## Phase 0 — Foundation *(done)*

- Tauri 2 application with a React + TypeScript frontend.
- Rust native core with module boundaries and a command layer.
- Connection layer shaped so USB and Wi-Fi share one pipeline (`Transport` seam).
- Light professional UI shell with dashboard and module registry.
- CI, documentation, ADRs, project metadata.

## Phase 1 — Device and connection *(done)*

- ADB integration: HermesGate bundles its own `adb` runtime (no Android SDK
  install), resolves it (override → bundled → `PATH` → SDK) and parses
  `adb devices -l` / `getprop` for state and metadata; one card per physical
  device, grouped by the most stable serial.
- Device Manager state exposed through the `list_devices` command and the
  `devices-changed` event, rendered in the Devices view.
- USB transport: connected-device sessions over the local ADB daemon.
- Wi-Fi transport: wireless pairing and discovery (mDNS / `adb pair`) with
  connect/disconnect from the UI.

## Phase 2 — Mirroring pipeline *(done)*

- Mirroring session established from an existing connection session
  (transport-agnostic by construction).
- Native decode (Media Foundation H.264 — software by default, opt-in hardware
  D3D11) and GPU presentation of the device screen.
- Live session telemetry in the dashboard: decode FPS, received bandwidth,
  decode/present timing, dropped frames, audio format.
- A bundled `scrcpy` server does the capture/encode work on the device side
  instead of a protocol written from scratch.
- Frame data stays in native code; the webview only receives state.

## Phase 3 — Input *(first slice shipped)*

Shipped:

- Navigation keys (back / home / recents) injected into the running session's
  control channel from the session panel.

Remaining:

- Pointer/touch, keyboard and gesture injection over the active session.
- Input intent from the UI through commands/events, never per-frame traffic
  through JavaScript.

## Phase 4 — Clean mirror window (OBS) *(done)*

- Separate window showing only the Android screen, with native desktop
  chrome (caption, resize borders, close box) so it behaves like a normal
  window and stays capturable by OBS window capture.
- Shares the single decoded output with the main preview (one decode, two
  presentations).

Remaining:

- Optional always-on-top; continue mirroring while the main window is
  minimized.

## Phase 5 — Clipboard, settings, packaging *(partly done)*

Shipped:

- Release packaging: Windows installers (MSI + NSIS) built by `tauri build` and
  attached to the GitHub Release; CI runs fmt, clippy and tests.

Remaining:

- Clipboard synchronisation between desktop and device.
- Persisted settings (transport preference, mirror window behaviour).
- Update channel.
