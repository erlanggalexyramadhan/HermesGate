# ADR 0001: Native core behind a narrow Tauri command surface

- **Status:** Accepted
- **Date:** 2026-10-07

## Context

HermesGate targets low-latency Android mirroring and control, but its UI is a
webview. Pushing video frames or per-input-event traffic through JavaScript
would cost latency and CPU, and giving the webview shell, filesystem, or device
capabilities would widen the attack surface.

## Decision

- Layering is `UI (React) → Tauri commands/events → Rust native core`. The
  webview holds `core:default` capabilities only
  (`src-tauri/capabilities/default.json`); no plugin permissions are granted.
- Every privileged capability is a Tauri command implemented in Rust
  (`src-tauri/src/commands.rs`, registered in `src-tauri/src/lib.rs`). The
  frontend calls commands only through typed wrappers (`src/lib/tauri.ts`).
- The native core is organised as explicit module boundaries
  (`src-tauri/src/core/`): device, connection, adb, mirroring, input, video,
  clipboard, settings.
- The future frame path is native end to end (decode + GPU render); the webview
  receives state and issues intents, never pixels.

## Consequences

- One auditable bridge between UI and native code; JavaScript can never reach
  shell, filesystem, or device APIs directly.
- Device logic can evolve (and the UI can reload) without coupling to the
  webview lifecycle.
- New capabilities require a command plus a typed wrapper — deliberate, small
  boilerplate at a single seam.
- A separate clean mirror window (ADR-adjacent, see `docs/architecture.md`)
  attaches to the native render output instead of the webview.
