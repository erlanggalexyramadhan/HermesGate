# Development

## Prerequisites

| Tool | Notes |
|------|-------|
| Rust (stable) | `rustup` from <https://rustup.rs>; `cargo`, `rustfmt` and `clippy` are enough |
| Node.js 20+ | npm is the package manager used by this repository |
| WebView2 runtime | Preinstalled on Windows 10/11; see <https://tauri.app/start/prerequisites/> for other platforms |
| Build tools (Windows) | Visual Studio Build Tools 2022 with the C++ workload + Windows SDK |
| Build tools (Linux) | `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev` (see Tauri prerequisites) |
| Build tools (macOS) | Xcode command line tools |

## First run

```bash
git clone https://github.com/erlanggalexyramadhan/HermesGate.git
cd HermesGate
npm install
npm run tauri dev
```

`npm run tauri dev` starts Vite on `http://localhost:1420`, compiles the Rust
core, and opens the HermesGate window. Frontend edits hot-reload; Rust edits
trigger a rebuild.

## Day-to-day commands

| Command | What it does |
|---------|--------------|
| `npm run dev` | Frontend only (browser); the native bridge is unavailable there and the UI says so |
| `npm run build` | Typecheck (`tsc`) + production frontend bundle into `dist/` |
| `npm run tauri dev` | Full development application |
| `npm run tauri build` | Production bundle (installer + binaries) |
| `cargo fmt --all` (in `src-tauri/`) | Format the Rust core |
| `cargo clippy --all-targets` (in `src-tauri/`) | Lint the Rust core |
| `cargo test` (in `src-tauri/`) | Run Rust tests (none yet) |

## Project structure

```
src/                     frontend (React + TypeScript + Vite)
  lib/tauri.ts           typed Tauri command wrappers
  App.tsx / App.css      shell, dashboard, design system
src-tauri/               Rust crate (`hermesgate`)
  src/lib.rs             Tauri builder, command registration
  src/commands.rs        command layer — the only frontend surface
  src/core/              native core boundaries (device, connection, adb,
                         mirroring, input, video, clipboard, settings)
  tauri.conf.json        window, CSP, bundling
  capabilities/          webview permissions (core:default only)
docs/                    architecture, roadmap, ADRs
.github/workflows/ci.yml CI: frontend build + Rust fmt/clippy/test
```

## Conventions

- **Commits:** [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `docs:`, `chore:`, …).
- **Frontend ↔ native:** every privileged capability is a Tauri command; add the
  command in `src-tauri/src/commands.rs`, register it in
  `src-tauri/src/lib.rs`, and expose a typed wrapper in `src/lib/tauri.ts`.
- **Native core:** module boundaries live in `src-tauri/src/core/`; a new module
  is added to the `MODULES` registry in `src-tauri/src/core/mod.rs` so it shows
  up in the dashboard.
- **Documentation:** architecture changes get an ADR in `docs/adr/`, and
  `README.md` / `docs/` must keep describing what actually ships.

## Troubleshooting

- **Port 1420 already in use** — a stale Vite process; end it, then rerun
  `npm run tauri dev`.
- **Rust not found** — reopen the terminal after installing `rustup`, or add
  `%USERPROFILE%\.cargo\bin` to `PATH`.
- **Blank window / CSP errors** — check the webview console; the CSP lives in
  `src-tauri/tauri.conf.json` (`app.security.csp` for release, `devCsp` for dev).
