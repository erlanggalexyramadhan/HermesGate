# HermesGate

**HermesGate — by Lex** is a desktop application for mirroring and controlling an
Android device, built for low-latency use cases such as content creation and OBS
workflows.

> **Status: Phase 1 (device detection).** This repository ships the application
> foundation — a Tauri 2 shell, a React + TypeScript UI, and a Rust native core —
> plus **USB Android device detection**: HermesGate bundles its own ADB runtime,
> enumerates attached devices with their state and basic metadata, and shows them
> in the Devices view. Screen mirroring, input injection and wireless pairing are
> **not implemented yet**; they are staged in the
> [roadmap](docs/roadmap.md).

## What exists today

- **Tauri 2 desktop shell** — main window, CSP, capability-scoped webview.
- **React 19 + TypeScript frontend** — dashboard UI that talks to the native core
  through typed Tauri commands only.
- **USB device detection** — the bundled `adb` runtime
  (`src-tauri/binaries/`, official platform-tools) enumerates attached Android
  devices; states (online / unauthorized / offline / disconnected) and metadata
  (serial, manufacturer, model, Android version) are kept by the Rust core and
  pushed to the Devices view through a Tauri event. No Android SDK install is
  required.
- **Rust native core** — module boundaries for Device Manager, Connection Manager
  (USB/Wi-Fi), ADB, Mirroring, Input, Video/Rendering, Clipboard and Settings,
  plus a module registry surfaced in the UI.
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

Frame data and input events are planned to stay entirely on the native side;
JavaScript is not part of the future video path. Details, including the shared
pipeline and the clean mirror window design, are in
[docs/architecture.md](docs/architecture.md).

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
├── docs/                 # architecture, development, roadmap, ADRs
└── .github/workflows/    # CI
```

## Documentation

| Document | Contents |
|----------|----------|
| [Architecture](docs/architecture.md) | Layering, module boundaries, transport-agnostic pipeline, mirror window design |
| [Development](docs/development.md) | Prerequisites, setup, day-to-day commands |
| [Roadmap](docs/roadmap.md) | Ordered phases from bootstrap to mirroring |
| [ADRs](docs/adr/) | Decisions that shape the foundation |

## License

[MIT](LICENSE) © Erlangga Lexy Ramadhan
