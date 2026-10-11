# HermesGate

**HermesGate — by Lex** is a desktop application for mirroring and controlling an
Android device, built for low-latency use cases such as content creation and OBS
workflows.

> **Status: v0.1.0 — mirroring works end to end (Windows).** The app connects to
> an Android device over **USB or Wi-Fi**, mirrors its screen into a separate,
> normal desktop window, plays its audio, and sends back / home / recents to the
> device. Clipboard sync, persisted settings, full pointer and gesture input, and
> platforms other than Windows are **not implemented yet**; they are staged in the
> [roadmap](docs/roadmap.md).

## What exists today

- **Tauri 2 desktop shell** — main window, CSP, capability-scoped webview.
- **React 19 + TypeScript frontend** — dashboard UI that talks to the native core
  through typed Tauri commands only.
- **Device detection (USB + Wi-Fi)** — the bundled `adb` runtime
  (`src-tauri/binaries/`, official platform-tools) enumerates attached Android
  devices; states (online / unauthorized / offline / disconnected) and metadata
  (serial, manufacturer, model, Android version) are kept by the Rust core and
  pushed to the Devices view through a Tauri event. No Android SDK install is
  required.
- **Mirroring** — the device screen is streamed with H.264 (a bundled `scrcpy`
  server is pushed over ADB), decoded natively with Media Foundation — software by
  default, opt-in hardware D3D11 decoding — and presented in a **separate mirror
  window** that carries only the picture, for OBS window capture.
- **Audio forwarding** — the device's audio stream is played on the desktop
  output through WASAPI.
- **Wi-Fi transport** — wireless debugging pairing and discovery (mDNS, `adb
  pair`) alongside USB, with disconnect in the UI, and **QR pairing**: the host
  shows the code, the phone's camera scans it.
- **Navigation keys** — back / home / recents are injected into the running
  session's control channel.
- **Live session telemetry** — decode FPS, received bandwidth, decode/present
  timing, dropped frames and audio format are shown while a session runs.
- **Rust native core** — module boundaries for Device Manager, Connection Manager
  (USB/Wi-Fi), ADB, Mirroring, Input, Video/Rendering, Audio, Clipboard and
  Settings, plus a module registry surfaced in the UI.
- **CI** — frontend typecheck/build and Rust fmt/clippy/test on every push and PR.
- **Documentation** — architecture, development setup, roadmap and ADRs.

## Architecture in one picture

```
┌──────────────────────────┐
│  React + TypeScript UI   │   webview: no shell / fs / device access
└────────────┬─────────────┘
             │  Tauri commands & events (typed, capability-scoped)
┌────────────▼─────────────┐
│   Rust native core       │   device · connection (USB/Wi-Fi) · adb
│                           │   mirroring · input · video · clipboard · settings
└──────────────────────────┘
```

Decoded frames and input never cross the webview: the UI is told about *state*,
never about pixels. Details, including the shared pipeline and the clean mirror
window design, are in [docs/architecture.md](docs/architecture.md).

## Getting started

Prerequisites: Rust (stable), Node.js 20+ and, on Windows, the WebView2 runtime
(full instructions in [docs/development.md](docs/development.md)).

```bash
npm install       # frontend dependencies
npm run tauri dev # development app (starts Vite + compiles the Rust core)
npm run tauri build # production bundle
```

## Repository layout

```
├── src/                  # React + TypeScript frontend
│   ├── lib/tauri.ts      # typed wrappers over the Tauri command layer
│   ├── App.tsx           # application shell / dashboard
│   └── App.css           # design system (light, professional)
├── src-tauri/            # Rust native core
│   └── src/
│       ├── lib.rs        # Tauri builder + command registration
│       ├── commands.rs   # command layer (the only frontend surface)
│       └── core/         # native core module boundaries
├── src-tauri/binaries/   # bundled adb (platform-tools) and scrcpy server
├── docs/                 # architecture, development, roadmap, ADRs
├── tools/bench/          # measurement scripts used during development
└── .github/workflows/    # CI
```

## Documentation

| Document | Contents |
|----------|----------|
| [Architecture](docs/architecture.md) | Layering, module boundaries, transport-agnostic pipeline, mirror window design |
| [Development](docs/development.md) | Prerequisites, setup, day-to-day commands |
| [Roadmap](docs/roadmap.md) | Ordered phases, with what is shipped marked |
| [PRD](docs/PRD.md) | Product requirements and acceptance criteria for v1 |
| [ADRs](docs/adr/) | Decisions that shape the foundation |

## License

[MIT](LICENSE) © Erlangga Lexy Ramadhan
