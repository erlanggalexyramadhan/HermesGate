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
(`src-tauri/src/core/`), and the module registry that the dashboard displays.
All behavioural modules are *planned*.

## Security boundary

- The webview runs with the `core:default` capability only
  (`src-tauri/capabilities/default.json`); no plugin permissions are granted.
- No shell, filesystem, or device capability is exposed to JavaScript. The only
  frontend entry points are the Tauri commands registered in
  `src-tauri/src/lib.rs`, each implemented in Rust.
- A strict CSP is configured for release builds (`app.security.csp` in
  `src-tauri/tauri.conf.json`), with a dev-only relaxation for Vite HMR.

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

## Future mirroring pipeline (*planned*)

```
Android
  capture / encode        (device side)
  transport               (USB or Wi-Fi — connection layer only)
  native decode           (video module, Rust)
  GPU rendering           (video module, native surface)
  HermesGate window(s)
```

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
- `src/App.tsx` — application shell: sidebar navigation (future sections are
  visibly disabled), dashboard content.
- `src/App.css` — design tokens and components; light, professional styling with
  no framework dependency.
