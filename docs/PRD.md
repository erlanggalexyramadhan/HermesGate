# HermesGate Product Requirements Document (PRD)

**Project**: HermesGate — Windows Android screen-mirroring application  
**Mode**: CAVEMAN + PONYTAIL  
**Minimum steps → minimum tokens → minimum cost → tetap aman & benar**

---

## 1. Mission

Develop HermesGate — a professional Windows Android screen-mirroring application inspired by scrcpy. Primary goals:
- Very low latency
- Wireless mirroring as a first-class feature, alongside USB
- Clean, floating mirror window suitable for OBS capture. **Superseded 2026-10-11 (owner instruction):**
  the window is no longer *borderless*; it is a normal movable desktop window (title bar, resize,
  close). The picture area stays free of overlays
- Reliable connection, input forwarding, and reconnection
- Low CPU, GPU, RAM, and bandwidth overhead
- Simple operation for non-technical users

Do not stop at planning or documentation. Inspect the real project state and implement the highest-priority unfinished milestone.

---

## 2. Architecture and Implementation Decisions

### Technology Stack
- **Frontend**: Tauri 2, React 19 + TypeScript
- **Rust core**: Native boundary for OS, device, and pixel operations
- **Capture/encode**: Android scrcpy server (v5.0, bundled in `binaries/`)
- **Video decode (Windows)**: Media Foundation H.264 MFT
  - Device (hardware) decode: **default**, D3D11 device manager and device
    textures; opt out with `HERMESGATE_H264_HW=0`
  - Software decode: fallback, system-memory BGRA frames (used when the decoder
    refuses the device or is not Direct3D 11 aware)
  - The `ProcessOutput` deadlock was a caller-side message-ordering bug, fixed
    (see ADR 0003)
- **Presentation**: Direct3D 11 video processor + DXGI swap chain **by default** (opt out with `HERMESGATE_VIDEO_GPU=0`); GDI presenter with `HALFTONE` as the fallback
- **Transport seam**: USB and Wi-Fi share one mirroring/input pipeline; `Transport` enum (`Usb`, `Wifi`)
- **ADR references**: `docs/adr/0001-native-core-behind-tauri-commands.md`, `0002-transport-agnostic-connection-layer.md`, `0003-device-decode-and-gpu-presentation.md`

### Module States (from `src-tauri/src/core/mod.rs` MODULES)
| Module | State | Responsibility |
|--------|-------|----------------|
| Device Manager | `ready` | Discover, identify and track connected Android devices |
| Connection Manager | `partial` | USB and Wi-Fi transports, wireless pairing and discovery |
| ADB Integration | `ready` | Single gateway to adb; no other module shells out |
| Mirroring Engine | `partial` | Session lifecycle for streaming a device screen (start/stop/status live; reconnect robustness open) |
| Input Engine | `partial` | Inject touch, key and gesture events into the device (pointer + Back/Home/Recents live; text and full keyboard absent) |
| Video & Rendering | `partial` | Native decode and GPU rendering of mirrored frames (GPU decode default and verified on one machine) |
| Clipboard | `planned` | Keep the desktop and device clipboards in sync |
| Settings | `planned` | Persisted application preferences |

---

### UI design alignment (2026-10-11)

Reference: `https://lance-speak-29635031.figma.site/` (a Figma Make "Device Console" mock). Its
tokens were read live from the served stylesheet, not guessed: `--bg #0a0c0d`,
`--top #121415`, `--rail #151719`, `--surface #171b1d`, `--surface-2 #121516`,
`--surface-3 #0e1011`, `--border #24292c`, `--border-soft #1b2022`, `--text #e4eaee`,
`--muted #707980`, `--faint #3e464b`, `--cyan #00a8ec`, `--green #00d395`, `--red #ff3d4f`;
Inter for text plus a mono face (JetBrains Mono there) for every label, badge and status line.

Adopted in `src/App.css`:

- The reference palette mapped 1:1. The previous token names are kept as aliases
  (`--bg-main`, `--card-bg`, `--accent-blue`, …) so no existing rule or component loses its token.
- The mono micro-label system, applied through one rule shared by `.panel-head h2`, `.card h2`,
  `.table th`, `.chip`, `.state`, `.stat-label`, `.stat-value`, `.device-id`, `.statusbar`,
  `.status-strip`, `.nav-tag`, `.sidebar-label` — at readable desktop sizes (9–12 px). The
  reference canvas is a scaled mock whose own text is 6–9 px, so its type scale was adopted, not
  copied literally.
- The reference's structural language: hairline borders (`#24292c` / `#1b2022`), 2–8 px radii,
  a 1 px cyan mark with a glow on the active nav item (mirroring `rail-button.active::after`),
  `#17323b` border-bottom tint on device/card surfaces, a divided telemetry grid of mono
  label/value cells (`.stats` / `.stat`), primary action button `#008fcc`, statusbar `#111314`.
- `src/App.tsx`: a shell statusbar (app + version, platform, core module count, current view)
  mirroring the reference's statusbar; a `Navigation` micro-label in the sidebar; the Devices
  view's contextual footer became `.status-strip` so the shell bar stays the single status bar.

Deliberately NOT copied: the reference's icon-only rail (icons would add a dependency, and the
labelled navigation must stay explicit) and its fabricated telemetry (device IP, uptime, encoder,
latency) — the app may only show fields the backend actually reports. Mirroring, wireless ADB,
the diagnostics panel, the borderless `WS_POPUP` mirror window and every existing behaviour are
unchanged; the change is presentation only.

Verification (2026-10-11): `npm run build` (tsc + vite) clean; the UI served from `vite dev` was
rendered in a browser and probed — body/sidebar/topbar/statusbar showed the mapped values
(`--bg #0a0c0d`, `--cyan #00a8ec`, `--border #24292c`, sidebar `#151719`, topbar `#121415`,
statusbar `#111314`), the build output carried the new tokens and class names, an in-page probe of
`.card`, `.panel`, `.panel-head h2`, `.chip`, `.device-card`, `.stats`, `.stat-label`,
`.stat-value`, `.status-strip`, `.table th`, `.btn-primary`, `.nav-item.is-active` and `.nav-tag`
resolved to the intended computed styles against the app's own stylesheet, and the page reported
zero errors. Boundary: the change was not re-verified inside the Tauri shell (frontend-only; a
release rebuild was not run for it).

---

## 3. P0 — Core Functionality Requirements

### Connection
- [x] Support USB ADB connections
- [x] Support wireless ADB pairing and connection — paired and connected on 2026-10-11 (`adb pair`, then
      `adb connect 192.168.18.7:39583`): the Wi-Fi device renders in the Devices view and streams, and a
      running session was switched USB -> Wi-Fi -> USB through the app's own device list (IDLE -> RUNNING
      in 19 s and 11 s); evidence in WORKLOG Milestone 12e
- [x] Handle device discovery, disconnects, reconnection, and connection errors — verified 2026-10-11 on
      device `bc058b2c`: killing the server on the device mid-session moved the session to `FAILED` with the
      reason `disconnected or stream ended`, the app and its process survived, and one click on **Start
      Mirror** returned it to `RUNNING` (WORKLOG Milestone 12g)
- [ ] Avoid unnecessary pairing steps after a device has been configured

### Mirroring
- [x] Display Android video with minimal latency (scrcpy server + raw_stream + TCP forward)
- [x] Forward the device's own playback audio to the desktop output — `audio=true audio_codec=raw` on the
      server plus a winmm `waveOut` render queue in `src-tauri/src/core/audio.rs` (48 kHz stereo 16-bit,
      8 x 4096-byte queue); verified live on 2026-10-11 against the phone's own playback (WORKLOG
      Milestone 12f)
- [ ] Use an efficient native video pipeline (Media Foundation + H.264 MFT)
- [ ] Avoid unnecessary frame copies, allocations, and cross-language transfers
- [ ] Handle resolution, orientation, and device changes safely
- [ ] Keep the implementation stable during prolonged sessions

### Video Decoding
- [x] Software decoding remains functional (default; BGRA frames via Media Foundation MFT)
- [ ] Optional hardware decoding works on supported hardware and fails safely
  - Controlled via `HERMESGATE_H264_HW` environment variable
  - Must fall back safely without breaking mirroring if the hardware path fails
  - Do not assume a hardware decoder works merely because it initializes
- [ ] Do not force hardware decoding by default
- [ ] Maintain a reliable software-decoding path always available

---

## 4. P1 — Reliability and Usability Requirements

- [x] Provide actionable error messages (each failure path returns specific reason strings)
- [ ] Make connection and decoding failures diagnosable
- [ ] Minimize CPU, GPU, memory, and network overhead
- [ ] Keep USB and wireless workflows consistent where possible
- [ ] Document prerequisites, limitations, and recovery steps
- [ ] Add only useful diagnostics; avoid excessive logging and sensitive data

### Mirror Window
- [x] Provide a clean, borderless, floating mirror window
- [ ] Keep application controls and overlays out of the captured mirror surface
- [x] Allow the main application window to minimize without unintentionally terminating the mirror session
      (verified at runtime on device `bc058b2c`, 2026-10-11 — evidence section below)
- [x] Ensure the mirror window is suitable for OBS window capture — the window carries no chrome: style
      bits read at runtime are `WS_POPUP` only (no `WS_CAPTION`, `WS_THICKFRAME`, `WS_BORDER`, `WS_SYSMENU`,
      exstyle 0) and its window rect equals its client rect (412x918); the client area carries the device's
      picture, re-measured 2026-10-11 back-to-back against `adb screencap` of the same screen: 2.27/255 mean
      absolute difference, 27.3 dB PSNR, and an alignment search over +-4 px x +-8 px finds its sharp
      minimum at offset (0,0) — no shift, crop or letterbox. One caveat, measured the same day: a
      `PrintWindow` reader with `PW_RENDERFULLCONTENT` does **not** reproduce this window's DXGI swap chain
      (13.3 dB PSNR, 91% black), so an OBS window-capture source must use the Windows Graphics Capture
      method (OBS's default on Windows 10 1903+) or display capture; a BitBlt/PrintWindow-style reader sees
      a partial picture
- [ ] Preserve the existing frontend unless changes are necessary to meet these requirements

### Input
- [ ] Forward supported mouse, keyboard, and touch interactions reliably — **pointer and
      navigation keys**: left press/drag/release and wheel scroll are implemented and verified live
      on device `bc058b2c`, and Back/Home/Recents key presses were added on 2026-10-11 and verified
      against the device's own `topResumedActivity` and view tree; free text and full keyboard input
      are not implemented and the UI offers no control for them
- [x] Respect Android permissions and platform limitations — injection goes through scrcpy's own
      server-side input APIs; no root, no Accessibility service, nothing installed on the device
- [ ] Report unsupported operations clearly — concrete verified gap: protected content (DRM video)
      mirrors as a black window with no explanation; `screencap` is refused for the same reason
      (`SurfaceFlinger: FB is protected: PERMISSION_DENIED`, 2026-10-11). Product decision needed
      on whether to surface a notice

### Lifecycle
- [x] Manage ADB processes, decoder resources, windows, and streaming sessions safely
- [ ] Avoid orphan processes, resource leaks, and unnecessary background activity
- [x] Handle application shutdown and device disconnection predictably — device disconnection verified
      2026-10-11 (session to `FAILED` with a specific reason, restart returns to `RUNNING`; WORKLOG
      Milestone 12g); shutdown cleanup via `Session::Drop` (criterion 11)

---

## 5. P2 — Performance Improvements

- [ ] Measure latency and resource usage before optimizing — **latency and CPU measured** 2026-10-11 (latency both ways: adb-path USB median 654 ms / Wi-Fi 746 ms, n=5, harness `tools/bench/latency.sh`; product-path USB median 698 ms / Wi-Fi 706 ms, n=8, harness `tools/bench/latency.ps1 -MirrorTitle` — see Acceptance criterion 3; CPU 1.2 CPU-s per 25 s of mirroring vs 4.8 for the scrcpy reference on the identical workload); **memory and GPU usage still unmeasured**, so this stays open
- [ ] Prioritize the largest demonstrated bottlenecks
- [ ] Avoid speculative optimization
- [ ] Record test conditions and distinguish measured results from estimates

---

## 6. Acceptance Criteria (track each as PASS, FAIL, BLOCKED, or NOT VERIFIED)

1. **USB connection works** — PASS (ADB integration, device listing, mirroring session start)
2. **Wireless pairing and connection work** — PASS (real `adb pair` + `adb connect 192.168.18.7:39583` on 2026-10-11; the Wi-Fi device rendered in the Devices view at runtime and `adb devices -l` still reports it as `device`; WORKLOG Milestone 7, task `t_cc527571`. Note: the pairing port was read from `adb mdns services` read-only — a full mDNS/SSDP discovery feature is still not implemented)
3. **Mirroring runs reliably with low latency** — PARTIALLY (frame rate: 51.1 FPS tail rate live on device `bc058b2c` over 25 s Settings-scroll runs, 3 runs, vs 49.9 FPS for the scrcpy v4.1 reference client on the identical workload. Latency: **measured** 2026-10-11 — a HOME press was injected with `adb shell input keyevent 3` and the clock ran to the mirrored 8x8 pixel block changing, harness `tools/bench/latency.sh <label> <serial> <samples>`: USB median **654 ms** (586/653/654/654/1403, n=5), Wi-Fi `192.168.18.7:39583` median **746 ms** (599/627/746/846/933, n=5), both with the display awake and no keyguard. Measured composition: the adb injection round trip alone is 190-249 ms (median 197 ms) and one sampler iteration is 13.3 ms, so the figure's own resolution is ~13 ms; the frame interval (19.6 ms at 51.1 FPS) is not the dominant term. **What the number is not:** not a per-stage decomposition, and not the latency of the app's own forwarded input — the injection leg went through `adb`, not through the session's control socket, so it is an upper bound on the user-visible path rather than a measurement of the product's own path.  The product's own path is measured too, 2026-10-11: the press is delivered through the session's own control socket by clicking the panel's **Home** button (the click is issued over CDP by `tools/bench/click_nav.mjs`, standing in for the human's mouse click, which the app cannot tell apart from one), harness `tools/bench/latency.ps1 -MirrorTitle 'HermesGate Mirror' -Clicker tools/bench/click_nav.mjs -Label Home`, same 8x8 block, same threshold, same host: USB median **698 ms** (605/620/650/662/698/701/713/733, n=8), Wi-Fi `192.168.18.7:39583` median **706 ms** (558/583/619/623/706/744/756/840, n=8), ~13 ms resolution, display awake, no keyguard. Both figures are **not** directly comparable to the adb one: that one carried a 197 ms adb floor and sampled the release `mirror_stats` example, this one has no floor and samples the app itself — and the product figure is not the lower of the two, so the residual between the click and the pixel change differs by ~240 ms (candidates: debug vs release build, control-socket injection vs `adb input keyevent`, WebView IPC). That gap is recorded as an open item, not as a claim. The `EVENT=screen` variant (median 813 ms) measures display reconfiguration and is not claimed as latency; mandate #7: no claim without measurement)
4. **Video uses an efficient native pipeline** — PASS (Media Foundation MFT -> D3D11 video processor -> DXGI swap chain is the default path; 1.2 CPU-s per 25 s of mirroring vs 4.8 CPU-s for the scrcpy reference client; software/GDI fallback kept; ADR 0003)
5. **Software decoding remains functional** — PASS (verified via Media Foundation MFT software decode, still selectable with `HERMESGATE_H264_HW=0`; offline tests pass 4/4 runs)
6. **Optional hardware decoding works on supported hardware and fails safely** — PARTIALLY (device decode verified live on one machine: `hardware=true`, D3D11 texture samples, 51.1 FPS and 30.6 dB PSNR at the default settings; the earlier `ProcessOutput` deadlock was a caller-side message-ordering bug, fixed and documented in ADR 0003; no second GPU/driver has been exercised; falls back to software on failure; mandate #7: do not assume hardware decoder works merely because it initializes)
7. **Mirror-only window is suitable for OBS capture** — VERIFIED (WS_POPUP borderless window at upper-left corner (0,0) eliminates title bars and frame; positioned at (0,0) so OBS window capture includes the full mirrored area without interference; and measured on 2026-10-11 against the device's own framebuffer: mirror window vs `adb screencap` of the same Settings screen, both scaled to 412x918, gave mean absolute difference 1.02/255, per-row RMSE 1.72, 43.4 dB PSNR, worst pixel 97, and no row band above 24 — no title bar, frame or letterbox to filter out). **Re-measured
2026-10-11 on the current build**, back-to-back with the device's own `screencap` of the same screen: window
rect equals client rect (412x918, so no non-client area at all), style bits read at runtime are `WS_POPUP`
only (no `WS_CAPTION`, `WS_THICKFRAME`, `WS_BORDER`, `WS_SYSMENU`; exstyle 0), 2.27/255 mean absolute
difference, 27.3 dB PSNR, and an alignment search over +-4 px x +-8 px finds its sharp minimum at (0,0) —
no shift, crop or letterbox. Caveat measured the same day: a `PrintWindow` reader with
`PW_RENDERFULLCONTENT` does **not** reproduce this window's DXGI swap chain (13.3 dB PSNR, 91% of pixels
black, content only in patches), so an OBS window-capture source must use the Windows Graphics Capture
method (OBS's default on Windows 10 1903 and later) or display capture; readers of the older
BitBlt/PrintWindow family see a partial picture)
8. **Minimizing the main window does not unintentionally terminate mirroring** — PASS (runtime, device `bc058b2c`, 2026-10-11: with a session `RUNNING` the main window was minimized via `SW_MINIMIZE` (`iconic_during=True`) and three `adb shell input swipe` events drove the device during the minimized period; the decoded-frame counter rose 3056 -> 3223 while the window was down, the mirror window kept its handle and stayed visible (412x918), and one `hermesgate` process ran before and after. One host/device only; evidence section below)
9. **Supported input forwarding works** — PARTIALLY (pointer only: left press/drag/release and wheel scroll are implemented over the scrcpy control channel (`control=true`) and verified live on device `bc058b2c` against the device's own view tree — taps opened the tapped rows, drags and wheel moved the list in the expected direction; a wheel-sign bug found and fixed the same session. Back/Home/Recents key presses were added on 2026-10-11 over the same channel and verified the same way — Home moved `com.android.settings/.OplusSettingsHomepageActivity` to `com.android.launcher/.Launcher`, Recents opened the task switcher (view tree: the Settings card plus `Tutup semua`), Back popped two distinct activities (`com.oplus.wirelesssettings/.SettingsActivity` -> `...SeriesInnerFeedActivity` -> `...SplashActivity`). Text and full keyboard injection are not implemented, so the criterion's "mouse, keyboard, and touch" is not yet fully met; evidence section below)
10. **Disconnect and reconnect behavior is reliable** — PASS (runtime, device `bc058b2c`, 2026-10-11: with a session `RUNNING` (30-36 decode FPS, audio stream open) the server process was killed on the device; the session reported `FAILED` with the reason `disconnected or stream ended` (verbatim UI text) within 2 s, the process stayed alive and the panel re-enabled **Start Mirror**, and one click returned the session to `RUNNING` (30.9 decode FPS) with the audio stream open again. One host/device and one failure mode (the server dying); a cable pull and an app restart are still untested — WORKLOG Milestone 12g)
11. **Application shutdown cleans up resources** — PASS (Session `Drop` impl kills server, removes forward; code compiles; verified)
12. **Errors are actionable and useful for troubleshooting** — PARTIALLY (specific reason strings in MirrorStatus.phase; could be more actionable; mandate #8: add only useful diagnostics; avoid excessive logging)
13. **Performance claims are backed by measurements** — PARTIALLY (stats counters exist in video.rs; 9-10 FPS measured early via software decode, since superseded by a measured 51.1 FPS tail rate on GPU decode, 43.4 dB mirror-vs-device PSNR and 1.2 CPU-s / 25 s, all on one host; mandate #10: all device-dependent tests marked NOT VERIFIED; mandate #7: do not generalize performance claims without device measurement; do not claim FPS/laten/kualitas telah terbukti tanpa pengukuran nyata)
14. **Documentation matches the implementation** — IN PROGRESS (this PRD.md audit update; mandate #2: keep docs concise and actionable; mandate #10: mark device-dependent results as NOT VERIFIED)
15. **Necessary checks pass, with remaining limitations explicitly recorded** — IN PROGRESS (cargo check passes; mandate #9: run only necessary checks; mandate #2: no redundant test suites)
16. **The device's audio is forwarded to the PC** — PASS (runtime, device `bc058b2c`, 2026-10-11: the session opens the server's audio socket (`audio=true audio_codec=raw`, no `audio_source` flag, so the server's playback-capture source is what arrives) and plays it on the desktop's waveOut render device at 48 kHz stereo 16-bit. Evidence: PCM arrives at 1540-1638 kbit/s against the 1536 kbit/s the format implies; `waveOutGetPosition` on the render device reports 3 s and then 6 s of played audio, advancing in step with wall-clock sampling; and the loudest sample in the 1 s window rises from **0 during silence to 1749-4591 (of 32767) exactly when the device is made to play** (two volume-key beeps, `adb shell input keyevent 24`, floor 0 in the samples before and after). The device's volume was restored by the symmetric key presses (2 up, 2 down). One host, one device; loudness was taken from the device's playback-capture path, and no second render device was exercised)

---

## 7. Current Status and Remaining Blockers

### Completed
- ADB integration with bundled adb binary (resolve, list_devices, device_props, getprop)
- USB mirroring session start/stop via scrcpy server with `raw_stream=true`, `control=false`, `tunnel_forward=true`
- Mirroring engine with session lifecycle (ACTIVE mutex, single session at a time)
- Video pipeline with the Media Foundation H.264 MFT — device decode + GPU presentation by default, software decode + GDI as the opt-out fallback (ADR 0003)
- GDI-based mirror window that works for OBS capture
- Resource cleanup on session end and app shutdown
- Wireless transport: `adb pair` / `adb connect` command layer, proven at runtime on 2026-10-11
  (paired, then connected to `192.168.18.7:39583`; the Wi-Fi device rendered in the Devices view
  and is still reported as `device` by `adb devices -l`; task `t_cc527571`, WORKLOG Milestone 7)
- Product statistics surface: in-app 1 Hz diagnostics panel over the existing counters (2026-10-11;
  decisions and verification in the section below)
- Pointer input forwarding over the scrcpy control channel (`control=true`, `tunnel_forward=true`):
  left press/drag/release and wheel scroll, verified live against the device's own view tree
  (2026-10-11; evidence section below)
- UI aligned with the reference design: reference palette/tokens plus its mono micro-label and
  component styling applied to the existing views (2026-10-11; see "UI design alignment" in
  section 2 and WORKLOG Milestone 9)
- Audio forwarding: the device's own playback, carried to the PC as raw 48 kHz stereo 16-bit PCM and
  played by a winmm `waveOut` queue in `src-tauri/src/core/audio.rs`; verified live on device `bc058b2c`
  (2026-10-11; WORKLOG Milestone 12f)
- Disconnect handling: a dead server on the device moves the session to `FAILED` with a specific reason
  and one click restarts it to `RUNNING`; verified live on device `bc058b2c` (2026-10-11; WORKLOG
  Milestone 12g)

### In Progress / Partially Complete
- Device decode + GPU presentation are the default and fail safe to software decode + GDI (measured on one device and GPU: 51.1 FPS, 1.2 CPU-s per 25 s, 30.6 dB; ADR 0003)
- Mirror window is a **normal movable desktop window** and suitable for OBS: `WS_OVERLAPPEDWINDOW` plus `AdjustWindowRectEx`, so the client area keeps the exact frame size the device sent and the OS draws the caption, the resize borders and the close box. Read off the live window (2026-10-11, `tools/bench/hg-window.ps1`): style `0x14cf0000` — `WS_CAPTION` yes, `WS_THICKFRAME` yes, `WS_SYSMENU` yes, `WS_POPUP` no; frame 428×957 against a 412×918 client (16×39 px of non-client chrome); `WM_NCHITTEST` on the caption returns `HTCAPTION` (draggable); `SetWindowPos` moved it (0,0) → (200,150). Pixel parity against the device's own `screencap`: 2.27/255 mean absolute difference, 27.3 dB PSNR, sharp zero-offset alignment minimum; the capture-method caveat (use Windows Graphics Capture or display capture, not PrintWindow/BitBlt) is recorded under criterion 7
- Error messages are specific but could be more actionable

### Not Started
- Input beyond the pointer: keyboard and free-text injection (Back/Home/Recents keys shipped 2026-10-11)
- Hardware decoding on a second GPU/driver (one device and GPU verified; a status line once suggested the peer display state changed the decode path — measured false, it printed before the output type was negotiated)
- Mirror window always-on-top behaviour
- Scroll magnitude per wheel notch: one notch is today one full scrcpy step, which reads as a large
  jump on a long list — a product decision, not a defect

### Device page — one device, one card, one button (2026-10-11)

Owner instruction: the Device page is the whole workflow for a non-technical user. What is now true of
the running application (verified live over CDP, device `bc058b2c`):

- **One card per phone.** `adb devices -l` lists this phone three times (its hardware serial, the mDNS
  alias of the same USB link, and `192.168.18.7:39583` over wireless). The Rust core groups those by the
  device's own `ro.serialno`, so the page shows exactly one card —
  `CPH2773 · ONLINE · bc058b2c · OPPO · Android 14 · USB + Wi-Fi` — and the status strip reads
  `Devices: 1 known`. Before the fix the same phone produced two cards and no manufacturer or Android
  version (the `getprop` parse expected `]:[`, the device prints `]: [`).
- **One transport picker, inside the card.** `Connection: [USB] [Wi-Fi]` selects the endpoint the next
  start will use; the line under it spells the resolution out (`USB via bc058b2c`,
  `Wi-Fi via 192.168.18.7:39583`). The picker is disabled while a session is live, so it can never
  silently change the connection under a running mirror.
- **One contextual button.** `Start Mirroring` → `Stop Mirroring`, with `Starting…`/`Stopping…` while
  the phase is in flight. It starts the real session over the *selected* endpoint and stops it:
  Wi-Fi start = `192.168.18.7:39583`, 720×1604, 40.7 decode FPS, 8.8 Mbit/s, audio 48 kHz; USB start =
  `bc058b2c`, 720×1604, 30.8 decode FPS, 6.8 Mbit/s; both return to `IDLE` on stop. A session belonging
  to another device leaves the button disabled with the reason in the hint line rather than a false
  start.
- **A device that is gone is not shown as a card.** Transports are refreshed on every poll and a phone
  reachable no transport is dropped from the list; a phone that loses USB keeps its card with Wi-Fi
  still selectable (unit test `losing_one_transport_keeps_the_card_and_the_other_transport`).
- **The mirroring navigation entry is gone**; mode selection, connection selection and start/stop all
  live on the Device page. Settings and the module table stay on the Dashboard.

### Product statistics surface — decisions and minimum implementation (2026-10-11)

Owner decisions, which close the five questions the earlier draft left open:

- **Surface — a diagnostics panel in the application UI.** Not an overlay on the mirror window:
  that window's client area is the phone picture (acceptance criterion 7), so an in-window overlay
  would be recorded along with it. (Since 2026-10-11 the window is a normal overlapped window
  rather than a borderless `WS_POPUP`, which does not change this: the picture is still the whole
  client area.)
- **Refresh — at most 1 Hz, pushed through the existing status channel** (`mirror_status` command
  + `mirror-changed` event). The session thread samples the counters where they are produced; the
  status the UI already polls carries them. No new channel, no new polling loop.
- **Data — the counters that already exist** (`Stats` in `src/core/video.rs`). No new
  instrumentation, no new pipeline work.
- **Frame overwrite — shipped as `Overwritten`**, the existing `frames_overwritten` counter
  (delta delivered − delta taken: frames handed to the presenter that a newer frame superseded
  before it painted). It is not labelled as total `dropped`, because no such counter exists.
- **Latency — still not shown in the panel, and that is deliberate.** An input-to-mirror figure is
  now measured offline (`tools/bench/latency.sh`: USB median 654 ms / Wi-Fi median 746 ms, n=5,
  2026-10-11 — see Acceptance criterion 3), but displaying one in-product would need instrumentation
  that attributes time to stages, and the app holds only the existing counters; a frame interval is
  not a latency. Stage times only, labelled by stage, with the panel's "not input-to-mirror latency"
  note left in place.
- **CSV — unchanged and still opt-in** (`HERMESGATE_VIDEO_STATS=<path>` writes the same counters
  at 1 Hz). One definition produces both the CSV row and the UI sample
  (`VideoStats::from_deltas`), so there is no second source of truth.

Implemented (2026-10-11):

- `src/core/video.rs`: `VideoStats`, `video::stats()`, `video::publish()`. The session thread
  samples once per second, publishes the sample for the product, then writes the CSV row when one
  was requested. The counters are always live (relaxed adds); the CSV stays the opt-in part. The
  sample is retracted when the session ends, so a UI can never read a finished session's numbers
  as live.
- `src/core/mirroring.rs`: `MirrorStatus.stats: Option<VideoStats>`, attached only while a session
  is `running` — the engine never hands a stale sample to the status channel.
- `src/App.tsx` + `src/lib/tauri.ts`: "Session diagnostics" panel in the app UI — Decode FPS,
  Received (Mbit/s), Decode (ms/s), Present (ms/s), Overwritten, Frames decoded — each labelled by
  the stage it measures, with an explicit "not input-to-mirror latency" note. Existing components
  and CSS classes only; no new dependency.

Verification (2026-10-11): `cargo test --release` — 18 unit + 2 integration pass, 1 ignored;
`cargo check --all-targets` clean; `npm run build` (tsc + vite) clean. `tests/video_stats_live.rs`
drives a real session from a loopback socket (no phone, therefore nothing to unlock) and pins both
the 1 Hz publish and the retraction at the end; on the 720x1604 sample it published
`fps 17.0 | 5.70 Mbit/s | decode 41.6 ms/s | present 1.5 ms/s | overwritten 28 | decoded 29` and
retracted the sample at `StreamEnd::Disconnected`. A serde contract test pins the JSON keys the
webview reads.

Note for a freshly added integration test: the shipping profile sets `panic = "abort"`, so a new
test target must run with `CARGO_PROFILE_RELEASE_PANIC=unwind` when using `--release`, otherwise
the `abort` dependency artifacts cannot link into the `unwind` test crate.

Blocker on the device leg: the phone harness was not run — the device was `Asleep` with
`isKeyguardShowing=true` at the time, and the harness must not run against a locked screen.
`cargo build --release --example mirror_stats` now prints the published sample, so the next
unlocked run confirms it end to end on real hardware.

### Pointer input forwarding — scope, protocol, verification (2026-10-11)

Implemented scope is **pointer and navigation keys**, stated as such in `src/core/input.rs`:
left-button press / move / release, wheel scroll, and Back/Home/Recents key presses
(`INJECT_KEYCODE`, type 0, as a `DOWN`/`UP` pair). Free text and full keyboard injection are not
implemented, and the UI offers no control for them.

- **Channel.** The session opens the scrcpy control channel (`control=true`, `tunnel_forward=true`).
  `Controller` connects the *second* socket of the session forward; the server accepts the video
  socket first and the control socket second, and blocks in `accept()` until it arrives.
- **Protocol.** The exact frames the Android server decodes (`app/src/control_msg.c`):
  `INJECT_TOUCH_EVENT` (type 2, 32 bytes) and `INJECT_SCROLL_EVENT` (type 3, 21 bytes),
  big-endian. Both carry `x, y, picture_width, picture_height`, so the device does the scaling and
  no client-side scaling is trusted. Scroll axes are fixed point in `[-1, 1]`, one wheel notch = 1.0,
  the same as the scrcpy client.
- **Coordinates.** The client area shows the whole picture; window coordinates map linearly onto
  picture pixels and are clamped, so a captured drag leaving the window stays inside the screen
  (`input::map`).
- **Wheel sign — fixed this session.** `video.rs` sent `-notches`. scrcpy sends `vscroll = event->y`
  unnegated (`sc_input_manager_process_mouse_wheel` ->
  `sc_mouse_processor_process_mouse_scroll` -> `SC_CONTROL_MSG_TYPE_INJECT_SCROLL_EVENT`).
  Measured on the device: the negated form scrolled the list the wrong way. Now `+notches`.

**Method.** Device state was read from the device itself — `uiautomator dump` (real view tree with
device-pixel bounds) and `dumpsys activity activities | grep topResumedActivity` — before and after
every desktop input, which was sent to the mirror window with `PostMessage`
(`WM_LBUTTONDOWN/UP`, `WM_MOUSEMOVE`, `WM_MOUSEWHEEL`) at coordinates scaled from the 720x1604
picture onto the 412x918 client area.

| Input (device pixels) | Observed device result |
|-----------------------|------------------------|
| tap, Bluetooth row centre (384,1070) | top activity -> `com.oplus.wirelesssettings/.SettingsActivity` |
| tap, Wi-Fi row centre (384,860) | Wi-Fi page: `Aktif`, saved network `LEX`, `Cumi - Cumi`, `Tambahkan jaringan` |
| tap, `Chrome` in the app list | app SubSettings: `Buka`, `Paksa berhenti`, `Nonaktifkan` |
| drag up (360,1400) -> (360,700) | list moved to later rows (`Aplikasi`, `Keamanan & privasi`, ...) |
| drag down (360,700) -> (360,1400) | list returned to the top rows (`Wi-Fi`, `Jaringan seluler`, `Bluetooth`) |
| wheel down, one notch at a time | app list stepped forward: `Browser` -> `F-Droid` -> `Gboard`; wheel up stepped back |

A screenshot was deliberately not used as evidence, and neither was "the command was sent": both
were green while the wheel was still inverted.

**Live session numbers at the same time** (in-app 1 Hz panel, session `RUNNING`): 720x1604,
51.4 FPS while scrolling, 2.6-2.9 Mbit/s, decode 35.3 ms/s, present 2.4 ms/s, 2878-3290 frames
decoded; log line `mirror: AMD Radeon RX 6750 XT for GPU decode and presentation` on this host.

**Navigation keys (added 2026-10-11).** `INJECT_KEYCODE` (type 0), 14 bytes big-endian —
`type(1) action(1) keycode(4) repeat(4) metastate(4)`, the layout scrcpy v5.0's serializer writes,
the version this app ships — sent as a `DOWN`/`UP` pair per press. Checked against the device's own
state, not against a "command sent" log, with the session `RUNNING 720 x 1604`:

| Key press from the session panel | Observed device result |
|----------------------------------|------------------------|
| Back | `com.oplus.wirelesssettings/.SettingsActivity` -> `com.ss.android.ttmd.video/...SeriesInnerFeedActivity` -> `.../SplashActivity` — two distinct pops |
| Home | `com.android.settings/.OplusSettingsHomepageActivity` -> `com.android.launcher/.Launcher` |
| Recents | `com.android.launcher/.Launcher` with the view tree showing the Settings task card and `Tutup semua` — the task switcher, not the home screen |

The three buttons were enabled only while a session was running, and every call returned without an
error (`no mirror session is forwarding input` would have surfaced in the UI banner).

**Limits.** One device, one GPU, one Windows host. Text and full-keyboard injection absent. Per-notch
scroll magnitude is one full scrcpy step (product decision). The mirror window was not captured
pixel-for-pixel against `screencap` in this session: the first attempt ran while the device was
playing protected video, where `screencap` is refused outright (section below).

### Protected content — `screencap` refused, mirror black (2026-10-11)

Found while attempting the mirror-versus-device pixel comparison. The device was playing a video
(`com.ss.android.ttmd.video/.series.innerfeed.activity.SeriesInnerFeedActivity`) and:

- `adb exec-out screencap -p` returned **0 bytes**, twice; `screencap -p /sdcard/...` wrote a
  **0-byte** file, so it is not an `exec-out` binary-mangling artefact.
- The mirror window's client area captured **412x918, mean luminance 0.63/255** with only 18 of 918
  rows above 8 — i.e. black, on two consecutive captures (929 ms apart), while the session reported
  ~30 FPS and the decoded-frame counter kept climbing (17252 -> 17929). Frames are arriving; their
  content is withheld.
- Root cause, from the device log: `W SurfaceFlinger: FB is protected: PERMISSION_DENIED`. The
  framebuffer is protected, so SurfaceFlinger refuses to hand it to the capture caller — and the
  same protection leaves the display-capture path (which the scrcpy server uses) black.

Evidence that this is content-specific, not a HermesGate defect: with a normal screen in front
(Settings), the same `screencap` produced a valid 67,842-byte PNG minutes earlier (17:59), and the
capture method (raise window, `CopyFromScreen` on the client area) is the one the existing bench
harness uses to compute PSNR against the device.

Consequences: (a) criterion 7's pixel-level comparison must be run on a non-protected screen — done
2026-10-11, see below; (b) a user watching protected video saw a black mirror with no explanation.
Gap (b) is now closed for the observation the client can actually make, and only for that:

**The app explains the empty mirror (2026-10-11).**

- **What is measured.** The window thread samples the picture it is about to present, at most once a
  second: mean luminance over a bounded pixel sample (`CONTENT_LUMA_FLOOR = 6.0/255`,
  `CONTENT_SAMPLES = 4000` in `src/core/video.rs`). BGRA frames are sampled directly, decoded GPU
  textures are read back. A probe that cannot be taken counts as **content** — a notice must never
  appear because a measurement failed — and the failure is logged once.
- **What is reported.** `MirrorStatus.picture_blank_secs` carries the wall-clock length of the
  current blank run (`0` while anything is visible, `0` for a session that is not running). After
  2 s the diagnostics panel shows a notice that states the measurement, the elapsed time, and the
  ambiguity: protected content and a display that is off look identical from the client, so it
  claims no cause.
- **Why not `isSecure`.** On this ColorOS build `dumpsys window | grep -c 'isSecure=true'` returns 0
  while the framebuffer is demonstrably protected, so that property cannot carry the decision. The
  measured picture is the only evidence the client owns; the `FB is protected` line is device-side.
- **Verified end to end** (device `bc058b2c`, session `RUNNING`): with the launcher in front there
  was no notice for 24 s at 30.4 FPS / 7.7 Mbit/s; with the protected video in front the notice was
  present on 20 consecutive polls, first appearing 4-5 s after the picture went blank, and the
  mirror capture at that moment measured 412x918 mean luminance 0.63/255; pressing HOME cleared it
  within 3 s. `cargo test --lib`: 29 passed, including a probe test that separates blank,
  near-black, dim and text-on-black pictures.
- **Fixed during that verification.** `publish()` cleared the blank run on every call while the
  sampler calls it with `Some` once per second, so the run could never pass one second and the
  notice never appeared. It now clears only when the sampler retracts (`None`).

### Key Blockers
- **Wi-Fi transport**: pairing and connect work through `adb pair` / `adb connect`; automatic mDNS/SSDP discovery is still not implemented; shares the pipeline by construction
- **Hardware decode**: the default path is device decode + GPU presentation with a software/GDI fallback (ADR 0003 — the earlier `ProcessOutput` deadlock was a caller-side message-ordering bug, fixed); a second GPU/driver is still unexercised
- **Input injection**: pointer (press/drag/release + wheel) and the navigation keys (Back/Home/Recents) are implemented and verified live; text and free-keyboard injection are not
- **OBS compatibility**: the borderless `WS_POPUP` mirror window at (0,0) is in place and minimizing the main window is now verified at runtime; always-on-top behaviour is still open — and the pixel-level parity check against `screencap` now measures 43.4 dB PSNR / 1.02 mean absolute difference on a non-protected screen (2026-10-11); always-on-top behaviour remains open
- **Protected content**: DRM video mirrors as black (device-side, `FB is protected`); the app now reports the measured blank picture instead of claiming a cause — distinguishing DRM from a display that is off is not possible from the client, so it is not claimed

---

## 8. Efficiency Rules

- Minimum steps, tokens, cost, and unnecessary tool calls
- Deterministic-first: inspect before changing
- Use installed skills when relevant
- No subagents unless clearly worthwhile
- No repeated tests without a reason
- No unbounded retry loops
- No unnecessary installations, downloads, or full rebuilds
- Do not ask questions when the repository and documentation provide enough information to proceed
- Ask only when a decision is genuinely blocking or a potentially destructive action requires authorization
- Keep all progress and final reports concise

---

## 9. Telegram Report Format

At the end of each meaningful execution session, report in Indonesian:

```
STATUS: Completed / In progress / Blocked

PRD: Created or updated; path

CHANGE: Important code and behavior changes

VERIFICATION: Checks actually performed and results

ACCEPTANCE: Pass / Fail / Blocked / Not verified

COMMIT: Commit hash, or why no commit was made

BLOCKER: Exact blocker, or none

NEXT: Single highest-priority next action

Avoid long logs, redundant explanations, and unsupported success claims.
```

---

## 10. Start Now

Inspect the actual repository and relevant Obsidian notes. Identify and use applicable installed skills. Update "docs/PRD.md", then implement the highest-priority incomplete milestone.

Do not stop after planning. Do not wait for unnecessary confirmation. Do not claim success without evidence. Continue until the available work is complete or a genuine blocker prevents progress.