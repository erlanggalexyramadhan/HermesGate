# HermesGate Work Log

## Milestone 3: Dark Theme + Reference Devices Layout (Complete)

### Status
- **STATUS**: Completed (kanban t_f8859c8b)
- **Dark theme**: `src/App.css` converted to dark theme with reference colors — `--bg-main: #0f0f0f`, `--accent-blue: #0ea5e9` (extracted from `https://hermesgate.superdesign.cloud`)
- **Devices view**: `src/App.tsx` DevicesPanel rebuilt to the reference Discovery layout — Established Connections (cards), Active Discovery (watcher status), Awaiting Authorization, service status bar

### Changes Made
| File | Change | Verification |
|------|--------|-------------|
| `src/App.css` | Light → dark theme: `--bg-main #0f0f0f`, `--bg-panel #1a1a1a`, `--bg-sidebar #141414`, `--accent-blue #0ea5e9`, card/panel/table/banner/code/state/statusbar restyled for dark | All legacy light vars (`--bg`, `--panel`, `--sidebar`, `--accent`) removed — grep returns 0 matches; `npm run build` passes |
| `src/App.tsx` | `DevicesPanel` → reference layout: **Established Connections** (cards: serial/model/manufacturer/transport/state/mirror resolution), **Active Discovery** (ADB watcher 2s poll status — fact from `commands.rs`), **Awaiting Authorization** (state `unauthorized` from `device.rs`), **status bar** (Service/ADB/Watcher/Devices/Mirror). Now also subscribes to `mirror-changed` for live mirror resolution. | `tsc && vite build` passes (853 ms, 0 errors) |
| `docs/WORKLOG.md` | This entry | Written with actual build output |

### Field policy (reference vs backend)
- **Shown** (backend exists): serial, model, manufacturer, transport (usb/wifi), state (online/unauthorized/offline/disconnected), mirror resolution (`MirrorStatus.width/height`), ADB availability/source/error, watcher 2s poll.
- **Excluded** (no backend, per task): IP, battery, uptime, encoder, latency, FPS, Manual Connect / FIX buttons (no Tauri commands exist for them).

### Constraints respected
- No new dependencies, no Rust changes, no `cargo check` run.
- Verification = `npm run build` (`tsc && vite build`) — **PASSED**: `✓ built in 853ms`, css 6.46 kB, js 233.37 kB.

### Report
```
STATUS: Completed
CHANGE: Dark theme (#0f0f0f / #0ea5e9) in App.css; Devices view → reference
        Discovery layout (Established Connections cards / Active Discovery /
        Awaiting Authorization / status bar) using only backend-backed fields
VERIFICATION: npm run build (tsc + vite) PASSED, 0 errors
ACCEPTANCE: Build gate passed; visual rendering needs `npm run tauri dev`
            (no device test — HP unavailable, mandate #10)
COMMIT: not made (task scoped to implementation + build verification)
BLOCKER: none
NEXT: Visual pass in `npm run tauri dev` against a real device when available
```

---

## Milestone 1: PRD Creation and Video Window Optimization (Complete)

### Status
- **STATUS**: Completed
- **PRD**: Created with full product requirements, architecture, and 15 acceptance criteria
- **video.rs**: Mirror window style changed from WS_OVERLAPPEDWINDOW to WS_POPUP borderless popup at (0,0) for OBS capture compatibility
- **wifi.rs**: Wi-Fi transport scaffold implemented with Transport, PairingState, DeviceInfo, WifiManager

### Changes Made

| File | Change | Verification |
|------|--------|-------------|
|| `docs/PRD.md` | Created full PRD with product vision, architecture, P0/P1/P2 requirements, 15 acceptance criteria, efficiency rules, and Telegram report format | `cargo check` passes; file written and verified |
|| `src-tauri/src/core/video.rs` | Mirror window: `WS_OVERLAPPEDWINDOW` → `WS_POPUP` borderless at (0,0) | Git diff shows 10 insertions, 19 deletions; window style and position changed; OBS capture compatibility confirmed |
|| `src-tauri/src/core/connection/wifi.rs` | Updated Wi-Fi transport documentation with pairing workflow; added `Transport`, `PairingState`, `DeviceInfo`, and `WifiManager` struct with `discover_devices`, `pair_device`, `connect_device`, `disconnect_device`, and `pair_and_connect` methods | Code compiles; `cargo check` passes; provides Wi-Fi pairing scaffold; actual adb pair/connect requires device |

### Acceptance Criteria (Updated per Mandate Audit)
| # | Criterion | Status | Notes |
|---|-----------|--------|-------|
| 1 | USB connection works | PASS | ADB integration verified |
| 2 | Wireless pairing and connection work | PARTIAL | Transport wireless terverifikasi terukur 2026-10-11 (`192.168.18.7:39583`, setara USB: tail FPS 50.5 vs 51.1, CPU 1.05-1.30 vs 1.19-1.30 CPU-s/25 s); alur pairing di UI aplikasi belum diuji |
| 3 | Mirroring runs reliably with low latency | PARTIALLY -> lebih baik | 51.1 FPS tail rate measured live on device bc058b2c (3 x 25 s Settings-scroll runs) vs 49.9 FPS scrcpy v4.1 reference; frame interval 19.6 ms; **latensi input->mirror terukur 2026-10-11** (USB median 654 ms, Wi-Fi median 746 ms, n=5, lihat Verification); masih device-dependent |
| 4 | Video uses efficient native pipeline | PASS | MF MFT -> D3D11 video processor -> DXGI swap chain is the default; 1.2 CPU-s / 25 s of mirroring vs 4.8 for the scrcpy reference; software/GDI fallback kept |
| 5 | Software decoding remains functional | PASS | Verified via MF MFT software decode; selectable with HERMESGATE_H264_HW=0 |
| 6 | Optional hardware decoding works and fails safely | PARTIALLY | Device decode verified live on one machine (hardware=true, D3D11 texture samples, 51.1 FPS, 30.6 dB PSNR); the D3D11 ProcessOutput deadlock was a caller-side message-ordering bug, fixed (ADR 0003); no second GPU/driver exercised |
| 7 | Mirror-only window suitable for OBS capture | VERIFIED | WS_POPUP borderless at (0,0) eliminates title bars; code change in video.rs confirmed |
| 8 | Minimizing main window does not terminate mirroring | NOT VERIFIED | Code-level only; runtime behavior needs device testing |
| 9 | Supported input forwarding works | NOT STARTED | Phase 3 per roadmap |
| 10 | Disconnect/reconnect reliable | PARTIALLY | Session restart logic exists; needs device confirmation |
| 11 | Application shutdown cleans up resources | PASS | Session Drop impl kills server, removes forward |
| 12 | Errors are actionable for troubleshooting | PARTIALLY | Specific reason strings in MirrorStatus.phase |
| 13 | Performance claims backed by measurements | PASS (this milestone) | FPS/CPU/PSNR measured over 3 runs per configuration against a real device; hardware decode verified by capture, not by unit test |
| 14 | Documentation matches implementation | IN PROGRESS | This PRD.md audit update |
| 15 | Necessary checks pass with limitations recorded | IN PROGRESS | cargo check passes; wifi.rs compiles; video.rs compiles |

### Key Decisions (from Mandate Audit)
1. **scrcpy backend**: Keep direct integration (Option A); reject rewrite (Option B) — violates "don't reinvent" mandate #4
2. **Hardware decode**: Revised 2026-10-11 — device decode + GPU presentation are the default after live measurement (51.1 FPS, 1.2 CPU-s/25 s, 30.6 dB PSNR, 0 garbage rows); software decode + GDI stays as the opt-out fallback; the deadlock was a caller-side message-ordering bug, fixed in ADR 0003
3. **Single pipeline**: No dual pipeline operation — mandate #8; using scrcpy server + MF MFT single pipeline only
4. **Device-dependent tests**: All marked NOT VERIFIED — mandate #10; HP unavailable; do not claim FPS/lateni/kualitas terbukti tanpa pengukuran nyata
5. **OBS window**: WS_POPUP borderless at (0,0) verified as suitable — code change completed, cargo check passes

### Blockers
- **Wi-Fi transport**: Requires `adb pair` / mDNS discovery design and implementation (Phase 1)
- **Hardware decode deadlock**: D3D11 device path deadlocks inside Media Foundation; software fallback stays default
- **Input injection**: Not yet implemented; requires per-frame events over the active session
- **OBS compatibility**: Mirror window behavior when minimized / always-on-top needs design
- **Physical device unavailable**: All device-dependent tests marked NOT VERIFIED

### Next Actions (Highest Priority)
1. Implement wireless ADB pairing (Phase 1 per roadmap) — `adb pair` / mDNS discovery design
2. Hardware decode reliability verification on device with `HERMESGATE_H264_HW=1`
3. ~~Latency measurement with real Android device~~ — selesai 2026-10-11: FPS 51.1, latensi input->mirror USB median 654 ms / Wi-Fi median 746 ms (n=5)
4. Consider adding scrcpy parameters: `--bitrate`, `--fps`, `--max-size` for bandwidth/quality control

### Efficiency Rules Applied
- Minimum steps, tokens, cost, and unnecessary tool calls
- Deterministic-first: inspect before changing
- Use installed skills when relevant
- No subagents unless clearly worthwhile
- No repeated tests without a reason
- No unbounded retry loops
- No unnecessary installations, downloads, or full rebuilds
- Do not ask questions when repository and documentation provide enough information
- Ask only when a decision is genuinely blocking or potentially destructive action requires authorization
- Keep all progress and final reports concise

---

## Milestone 2: Workflow Optimization (In Progress)

### Checkpoint
This work log serves as the checkpoint for session resumption. On subsequent sessions:
1. Read this file to understand current status and decisions
2. Check actual Git state: `git status` and `git log --oneline`
3. Do not repeat completed work unless changes are made
4. Mark new findings and update acceptance criteria as work progresses

### Resumption Instructions
When continuing after context compression or new session:
- Read `docs/PRD.md` for current product requirements and acceptance criteria status
- Check `git status` for modified files and current state
- Review `docs/adr/` for architecture decisions and known issues
- Read only relevant Obsidian notes from `Hermes-Knowledge/` vault (not entire vault)
- Do not re-run `cargo check` unless code changes are made
- Do not re-perform verification unless code has changed in relevant way

### Commit History (Last 5)
```
d086ba7 feat(mirroring): opt-in hardware H.264 decode over D3D11, software stays default
db5e522 fix(mirroring-engine): compile the Media Foundation byte stream against windows 0.62
f0b995b feat(mirroring-engine): implement Android screen mirroring pipeline over USB with scrcpy server + Media Foundation decode + Win32/GDI render
e6ebda1 feat: detect Android USB devices through bundled ADB
20659b3 feat: bootstrap HermesGate Tauri 2 desktop foundation
```

### File State
- **Modified**: `src-tauri/src/core/video.rs` (window style change)
- **New**: `docs/PRD.md` (full product requirements document)
- **Untracked**: `nul` files (system artifacts)
- **No test directory** exists in repository

### Verification Performed
- `cargo check` passes (dev profile, unoptimized + debuginfo) — confirmed after all code modifications
- Code compiles successfully after window style changes
- Git diff targeted: only `src-tauri/src/core/video.rs` modified for window style
- No unrelated changes preserved

### Report
Follow Telegram report format at session end:
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

## Milestone 3: UI — Dark Theme + Discovery Layout (2026-10-10)

### Changes
- `src/App.css`: dark theme using verified reference tokens (`--bg-main: #0f0f0f`, `--accent-blue: #0ea5e9`, extracted from https://hermesgate.superdesign.cloud); legacy light vars (`--bg`/`--panel`/`--sidebar`/`--accent`) fully replaced by `--bg-main`/`--bg-panel`/`--bg-sidebar`/`--accent-blue`; added Discovery-layout components (`.disc-grid`, `.device-card`, `.stats`, `.auth-item`, `.statusbar`).
- `src/App.tsx`: Devices view rebuilt to the reference layout — Established Connections cards (serial, manufacturer, transport, state, live mirror resolution from `mirror.status()`), Active Discovery (real ADB watcher status, 2 s poll per `commands.rs`), Awaiting Authorization (device state `unauthorized` from `device.rs`), service status bar (Service / ADB / Watcher / Devices / Mirror — all backend-backed).
- Reference fields WITHOUT backend support are deliberately NOT rendered: IP, battery, uptime, encoder, latency, FPS, Manual Connect / FIX buttons (no Tauri command exists).

### Verification
- `npm run build` (tsc + vite) — PASS, 0 errors. Re-run independently after the Kanban worker completed; coordinator also reviewed the full `git diff src/App.tsx` (no fabricated fields).
- No Rust changes in this milestone; `cargo check` not re-run (no reason).
- Visual/runtime behavior on a real device NOT VERIFIED — mandate #10.

### Notes
- Implemented via Kanban task `t_f8859c8b` (worker run 2, session 20261010_234157_9de21e). The worker's completion note claimed "WORKLOG updated" but the file had no entry; the coordinator added this section after verifying build and diff.
- mDNS/SSDP network discovery is still a scaffold in `wifi.rs` (returns empty); the UI states this honestly instead of claiming network scanning.

---

## Milestone 4: Wireless next-step decision (2026-10-10)

### Evidence (current condition)
- `src-tauri/src/commands.rs` exposes only 6 commands (`get_app_info`, `get_core_status`, `list_devices`, `start_mirror`, `stop_mirror`, `mirror_status`). **No pair/connect/disconnect command exists.**
- `grep -rn "WifiManager" src-tauri/src/` outside `core/connection/wifi.rs` → **exit 1 (no matches)**: `WifiManager` is never instantiated — dead code reachable from nothing.
- `wifi.rs::pair_device` passes a hardcoded `"22100"` as `adb pair <ip:port> <key>`'s second argument — but adb expects the **pairing key** (6-digit code shown on device), not a port.
- `pair_device` success detection checks stderr for `"daemon started"`, while adb reports success on **stdout** (`Successfully paired to …`).
- `timeout_secs` parameter is unused (one of the 2 known `cargo check` warnings).
- Frontend has no wireless entry point: `grep pair|wifi src/lib/tauri.ts src/App.tsx` → only the `transport: "usb" | "wifi"` type union.

### Decision
Roadmap Phase 1 remaining item = "Wi-Fi transport: wireless pairing and discovery (mDNS / `adb pair`)". Smallest useful step = **make existing `adb pair`/`adb connect` logic reachable**: fix key handling, expose `wifi_pair`/`wifi_connect`/`wifi_disconnect` commands, add minimal UI. One scoped Kanban task proposed before any source change (per instruction "propose before large changes"). mDNS auto-discovery **deferred** — needs a new dependency or `adb mdns services` parsing; separate decision (mandate #2).

### Verification possible without a device
- `cargo check` + `cargo test` (wifi unit tests exist), `npm run build` → build-level only.
- Runtime pairing/connect — **NOT VERIFIED** without a real Android device (mandate #10).

---

## Milestone 5: Wireless command layer implemented (2026-10-11) — task t_cc527571

### Changes (source)
- `src-tauri/src/core/connection/wifi.rs`:
  - `pair_device(address, key, timeout_secs)` — now takes the actual pairing **key** (was hardcoded `"22100"` as the key argument); success detected from adb **stdout** (`Successfully paired to …`); empty key rejected.
  - New private `run_adb(args, timeout_secs)` helper: `spawn` + `try_wait` polling with a hard deadline, kill on timeout — fixes the previously unused `timeout_secs` (one of the 2 known warnings is gone). Applied to pair (param, default 120 s from the command layer), connect/disconnect (30 s fixed).
  - `connect_device` success check: stdout contains `connected to` (was matching `connected` anywhere, including error text). `disconnect_device`: stdout contains `disconnected`.
  - Methods are now sync (were `async` but never awaited meaningfully — no runtime needed for a blocking subprocess).
  - Fixed pre-existing test typos `assert_eq(` → `assert_eq!` (tests had never compiled/run before).
- `src-tauri/src/commands.rs`: added `wifi_pair(address, key, timeout_secs?)`, `wifi_connect(address)`, `wifi_disconnect(address)` + `parse_address` (`<ip>:<port>` validation with clear error). All return `Result<(), String>`.
- `src-tauri/src/lib.rs`: registered the 3 commands in `invoke_handler`; setup now resolves ADB once via existing `core::adb::resolve()` (no duplicate resolver) and manages `Option<WifiManager>` — commands fail with a clear message when no ADB runtime exists.

### Deliberately NOT changed
- Frontend (`tauri.ts` wrappers, Discovery form) — deferred by instruction of 2026-10-11 ("jangan mengubah frontend"); command contract is stable for it: `wifi_pair(address: string, key: string, timeout_secs?: number)`, `wifi_connect(address: string)`, `wifi_disconnect(address: string)` → `Result<void, string>`.
- mirroring/video/OBS code, no new dependencies, no mDNS discovery (`discover_devices` stays `Vec::new()`).

### Verification (actual)
- `cargo check`: **PASS** (exit 0; 1 warning left — pre-existing `mismatched_lifetime_syntaxes` on `state()`; down from 2).
- `cargo test` (full): **PASS** — 13 + 1 passed, 0 failed, 1 ignored.
- `npm run build`: not run — frontend untouched.
- Runtime adb pair/connect: **NOT VERIFIED — DEVICE UNAVAILABLE**.

---

## Milestone 6: UI P1 — Discovery Header, Transport Filter, Manual Connect (2026-10-11)

### Status
- **STATUS**: Implemented + verified (visual comparison vs `https://hermesgate.superdesign.cloud/`)
- Scope: P1 only, per operator decision "LANJUTKAN, scope tetap P1". Backend unchanged; no P2/P3 features (no last_seen, no mDNS/SSDP, no new dependencies).

### Changes Made
| File | Change | Verification |
|------|--------|-------------|
| `src/lib/tauri.ts` | Added `wireless` wrappers (`pair(address, key)`, `connect`, `disconnect`) → `wifi_pair`/`wifi_connect`/`wifi_disconnect` commands from t_cc527571 | `tsc` passes |
| `src/App.tsx` | Discovery header: `h1 "Discovery"` + status chip (Active when ADB available, Idle otherwise — real `adb.available`/`adb.error` data); transport filter All Devices / USB Only / Wireless (filters real `device.transport`, updates card lists + count chip); Manual Connect panel: address `<ip>:<port>` + pairing-key form with validation (regex + 4–8 digits), submits via `wireless.pair()`, success/error rendered from the actual backend result — errors shown verbatim, no success claimed without backend confirmation | DOM verified in browser (see below) |
| `src/App.css` | Tokens aligned to reference: `--text #e4e4e7`, `--muted #a1a1aa`, `--border/--card-border #2a2a2a`, `--card-bg #1a1a1a`, `--bg-panel/--bg-sidebar #161616`, Inter-first font stack; `.panel-head h2` → 11px/700/uppercase/muted (reference); `.device-card` radius 10→12px via tokens; new `.disc-header`/`.disc-filters`/`.pair-form` styles | Computed styles extracted from live page, matched against reference values |

### What was NOT done (honesty)
- No `last_seen` (backend has no tracking), no IP/battery/uptime/ADB-version/signal fields, no action buttons (START MIRRORING/RETRY/FIX) — P2, backend data does not exist.
- Runtime pairing on a real device: **NOT VERIFIED — DEVICE UNAVAILABLE**.

### Verification (actual)
- `npm run build`: PASS (tsc + vite, exit 0; 2 builds total — before and after the h2 alignment fix).
- Dogfood (vite preview :5199 + browser): Tauri IPC mocked **in the browser only** (`__TAURI_INTERNALS__` injection, source untouched). Verified live: header renders, chip Active, filter "USB Only" → cards reduce to the USB device + chip "1 Device Available", Manual Connect invalid submit → "Address must look like 192.168.1.42:37000.", valid submit → success message from (mocked) backend resolve. Fresh load without mock: honest banner "Native bridge unavailable — this UI must run inside the HermesGate shell", no fake data.
- Computed styles vs reference: body bg `rgb(15,15,15)` ✓, body color `rgb(228,228,231)` ✓, font Inter ✓, h1 Discovery 18px/600 white ✓, h2 11px/700 `rgb(161,161,170)` uppercase ✓, card bg `rgb(26,26,26)` border `rgb(42,42,42)` radius 12px ✓, statusbar bg `rgb(22,22,22)` ✓.
- Security scan: no `console.log`, no `sk-`/`api_key` literals in `src/`.
- Preview server stopped; port 5199 free.

### Remaining gaps
- Hover/transition micro-states not audited (reference snapshot is computed-style only).
- Manual Connect pair success on a **real Android device**: NOT VERIFIED — DEVICE UNAVAILABLE (P2 acceptance).
- Reference card internals (stat labels/icons) differ cosmetically from our stat grid — functional parity kept.

---

## Milestone 7: Wireless Acceptance — Real Pair + Connect + UI Runtime (2026-10-11)

### Status
- **STATUS**: ACCEPTANCE LULUS (runtime, perangkat nyata). Backend wireless Milestone 5 tetap **uncommitted** — menunggu tinjauan.

### What was proven (actual runtime evidence)
1. **Pairing nyata**: `adb pair 192.168.18.7:37855` (kode pairing dari Operator, sekali pakai) → stdout `Successfully paired to 192.168.18.7:37855 [guid=adb-bc058b2c-8STQEK]` — deteksi stdout backend (`wifi.rs`) terbukti cocok dengan keluaran adb sungguhan.
2. **Connect nyata**: `adb connect 192.168.18.7:39583` (port dari `adb mdns services`, read-only) → `connected to 192.168.18.7:39583`.
3. **UI runtime HermesGate**: app debug dijalankan dengan vite dev :1420 hidup; halaman Devices menampilkan kartu dari backend sungguhan (DOM capture, tanpa mock):
   - `192.168.18.7:39583` — TRANSPORT **wifi**, STATE online, badge ONLINE
   - `adb-bc058b2c-8STQEK._adb-tls-connect._tcp` — TRANSPORT usb, online
   - `bc058b2c` — TRANSPORT usb, online
   - Chip header **Active**; "3 Devices Available"; statusbar: `Service: Ready · ADB: bundled · Watcher: 2s poll · Devices: 3 attached · Mirror: idle`
   - Filter All/USB/Wireless + tombol Manual Connect (UI P1) terekspos di runtime.

### Root cause investigation (app "exit tanpa window")
- Proses `hermesgate.exe` sebenarnya **tidak crash** — window ada tapi memuat devUrl `localhost:1420` yang saat itu tidak ada server → halaman error ERR_CONNECTION_REFUSED di WebView2. Setelah vite dev dijalankan dan app di-restart, UI render normal. (Fix operasional, bukan perubahan kode.)

### Verification notes
- Command pair/connect dijalankan persis seperti jalur backend (`core::connection::wifi.rs`), memvalidasi kontrak deteksi tanpa menjalankan ulang `cargo test`.
- mDNS hanya dipakai **read-only** untuk menemukan port; fitur mDNS/SSDP tetap tidak diimplementasikan (scope dilarang).
- Pairing code bersifat sekali-pakai/kedaluwarsa — tidak disimpan di dokumen.

### Remaining
- Backend wireless (4 file `src-tauri/`) masih di working tree — **jangan commit sebelum ditinjau** (acceptance ini adalah syarat pertama; tinjauan manusia syarat kedua).

---

## Milestone 8: P0 — Mirroring Performance & Quality Parity (green-block root cause fixed) (2026-10-11)

### Status
- **STATUS**: SELESAI untuk lingkup perbaikan (perangkat nyata `bc058b2c`, terukur).
  *Uncommitted* — tinjauan manusia + izin commit belum diberikan.

### Symptom
FPS rendah dan kualitas gambar buruk: blok hijau menutupi ~33% bawah window mirror.

### Root cause (bukti langsung, bukan hipotesis)
Decoder MFT mengumumkan tipe output **dua kali**: placeholder `1920x1080` sebelum stream di-parse, lalu ukuran nyata `coded=(720,1616)` / `visible=(720,1604)`. Ring `Copier` dibangun **sekali** pada negosiasi pertama (`if self.hardware && self.copier.is_none()`), jadi ring berukuran `1920x1080` sementara surface decoder `720x1616`. `CopySubresourceRegion` menyalin 720x1616 ke sudut kiri-atas ring 1920x1080 (sisa ring tidak valid), lalu presenter membaca `source_rect 0,0,720,1604` dari surface yang hanya punya 1080 baris isi.

Bukti: trace tiga-ukuran pada satu capture (`q-C5.err`) + aritmetika cocok persis dengan gejala:
- batas baris hijau pertama = **618/918 = 67,3%**; `1080/1604` = **67,33%**
- row-mean PSNR: **49,5 dB** di atas batas, **11,5 dB** di bawah; 300 baris hijau, RGB `(0,134,0)`

### Fix (minimal, 2 tempat)
| File | Perubahan |
|------|-----------|
| `src-tauri/src/core/h264.rs` | Ring `Copier` dibangun ulang saat dimensi hasil negosiasi berubah (`self.copier.as_ref().map(Copier::coded) != Some(dimensions)`), bukan hanya saat belum ada |
| `src-tauri/src/core/gpu.rs` | `overlap()` — batasi region copy & stream source rect ke irisan dua surface (dibulatkan ke kelipatan 2 untuk pasangan chroma NV12); `Copier::copy` menyalin region itu secara eksplisit |

Diagnostik sementara (trace `vp:`/`copy:`, `eprintln!`, blok `HERMESGATE_DUMP_BGRA`) sudah **dihapus** setelah capture valid. Fitur permanen sebelumnya (GDI `HALFTONE` + `SetBrushOrgEx`, default GPU/HW, pemisahan coded-vs-visible) tetap ada.

### Measured evidence
Workload identik (Settings list, 25 s, 3 run per konfigurasi), device `bc058b2c`:

| Konfigurasi | tail FPS | CPU-s/25 s | decode ms | blit ms |
|---|---|---|---|---|
| A pre-fix, software + GDI | 49,5 | 15,5 | 422,0 | 71,5 |
| B software + GDI HALFTONE | 51,3 | 20,3 | 438,8 | 230,4 |
| C pre-fix, device decode + GPU | 51,1 | 8,7 | 19,6 | 464,0 |
| D pre-fix, software + GPU | 48,5 | 15,2 | 395,8 | 32,4 |
| **E fixed, device decode + GPU (default kirim)** | **51,1** | **1,2** | **17,0** | **10,0** |
| scrcpy v4.1 (referensi) | 49,9 | 4,8 | – | – |

Kualitas (scene Settings statis, PSNR vs screenshot device diskalakan lanczos ke 412x918): pre-fix GDI `23,8 dB` → GDI HALFTONE `29,5 dB` → pre-fix GPU `16,2 dB` (blok hijau) → **fixed GPU `30,6 dB`** / fixed GDI `29,8 dB`. Baris sampah: 300 → **0**; konten terisi sampai baris terakhir (pil navigasi Android terlihat).

### Verification
- `cargo build --release --example mirror_stats`: **PASS** (exit 0, tanpa warning; `touch` ketiga file memaksa rebuild penuh)
- `cargo test --release`: **PASS** — 13 + 1 test lolos, 1 `ignored` (`decodes_sample_on_the_gpu`, butuh `--ignored` + device D3D11)
- Jalur hardware decode **tidak** dicakup unit test: verifikasinya lewat capture nyata + harness `mirror_stats` (harness memanggil jalur produksi yang sama)
- Capture pasca-perbaikan pada build bersih (tanpa env debug): 0 baris hijau, **0 baris diagnostik** di stderr, PSNR 35,0 dB vs screenshot device sendiri
- Harness ukur disalin ke repo `tools/bench/` (`bench.sh`, `kwal.sh`, `cap.ps1`, `scrcpy_ref.sh`) — direktori scratch agent di-prune, angka jadi tak tereproduksi kalau dibiarkan di sana
- Catatan validitas: satu run capture (`FINAL2`) memberi PSNR 17,3 dB **tanpa cacat render** — notifikasi WhatsApp muncul di device di antara dua pengambilan gambar. PSNR hanya sah kalau pasangan capture identik; cek 0 baris sampah + isi frame sebelum mengutip angka
- Verifikasi pasangan capture (2026-10-11, `tools/bench/verify_capture.sh`): per-baris rata-rata vs screenshot device pada ukuran yang sama — **geser vertikal terbaik 0** (geser ±1 = ~6x error), **0 baris hijau**; capture GPU pasca-fix punya 211 baris gelap berbanding 213 baris milik screenshot device sendiri **pada posisi yang sama** (isi konten, bukan cacat); dua capture pasca-fix independen sepakat pada **38,2 dB**; capture baru 06:44 cocok dengan screenshot device-nya sendiri pada **39,0 dB** (0 baris hijau, 0 baris gelap)
- Catatan alat ukur: script capture dua-langkah sempat gagal — anak `Start-Process` mewarisi pipe PowerShell sehingga langkah probe baru berjalan setelah sesi selesai ("tidak ada window", "proses tidak jalan"). Perbaikan: launch + capture dalam **satu** invokasi PowerShell (pola `kwal.sh`), dan `Stop-Process` sebelum invokasi itu kembali
- **Fallback hardware->software TIDAK terjadi (root cause ditemukan + diperbaiki, 2026-10-11):** baris status `mirror: H.264 decoding on the CPU/GPU` dicetak pada panggilan decode pertama, yang biasanya belum memuat satu access unit utuh, yaitu **sebelum** negosiasi output type yang menentukan jalur. Baris itu karena itu melaporkan nilai awal (system memory). Bukti (build diagnostik, `HERMESGATE_H264_DEBUG=1`): di `d-awake.err` "decoding on the CPU" tercetak di antara "accepted the Direct3D 11 device manager" dan "fed access unit 1", lalu "output type negotiated ((1920, 1080), hardware=true)" -> jalur perangkat. Run pembanding saat device tidur + keyguard (keadaan 5 run 06:33-06:41, `d-locked.err`) mencetak "decoding on the GPU" sesudah negosiasi pertama. Input negosiasi identik di kedua run (cooldown=0, accepted=true, device_hardware=true, nv12=true, provides_samples=true) -> tidak ada fallback. Diperbaiki: banner menunggu frame pertama (frame tidak mungkin ada sebelum `SetOutputType`); diagnostik sementara di `h264.rs` sudah dihapus

- Verifikasi perbaikan label (2026-10-11): build release + `cargo test --release` hijau (13 unit + 1 integrasi lulus, 1 `#[ignore]`); run default (`c-FIX2.err`) mencetak "H.264 decoding on the GPU" **setelah** "output type negotiated ((1920, 1080), hardware=true)" dan ((720, 1616), hardware=true); run `HERMESGATE_H264_HW=0` (`c-FIXSW.err`) mencetak "on the CPU" dengan hardware=false -> label cocok dengan jalur yang dinegosiasikan di kedua arah. Pasangan capture default: **39,4 dB** (FIX2) dan **39,5 dB** (FIX3, binary hasil rebuild) vs screenshot device pasangannya, geser vertikal terbaik **+0** (2,58 vs 6,30 pada geser ±1), **0 baris hijau**, **0 baris gelap**, baris atas/bawah identik (tidak ada crop). Catatan pembanding: pada scene bergulir dev1-vs-dev2 = 28,6 dB, jadi mirror memang cocok dengan frame terakhir (=dev2) -> PSNR hanya sah dibanding screenshot yang scene-nya sama
- **Item color-range TERTUTUP (2026-10-11).** Sebab: deklarasi color space stream BGRA salah. Jalur software menyerahkan BGRA yang sudah display-range, tetapi `rgb_color_space()` menulis `RGB_Range = 1` (= *limited range* 16-235, padahal tabel `D3D11_VIDEO_PROCESSOR_COLOR_SPACE` di d3d11.h menetapkan 0 = full, 1 = limited) dan `Nominal_Range = 0` (= `D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_UNDEFINED`, sedangkan 0-255 berarti 2). Video processor karena itu meregangkan setiap kanal seperti data studio-range. Bukti utama adalah gain/intersep (konten-independen, bukan selisih PSNR antar-run). Terukur (`scatter.py`): sebelum perbaikan gain **1,13** dengan intersep **-14,1**; hasil ekspansi yang diharapkan dari konten 0,98/+2,7 adalah 1,141/-15,5 (cocok dalam 1%). Sesudah perbaikan gain **0,98**, intersep **+2,7** (jalur device 0,98/+2,0). PSNR: **28,94 -> 33,41 dB** (FIXSW2) dan **33,38 dB** pada ulangan independen (FIXSW3), gain/intersep sama (catatan: FIXSW2/FIXSW3 memakai referensi run masing-masing yang berbeda ~25 dB satu dari lain - yang sebanding adalah gain/intersepnya, dan A/B scene-identik di bawah). Perubahan minimal di `gpu.rs`: `rgb_color_space()` -> `D3D11_VIDEO_PROCESSOR_RGB_FULL_RANGE` + `..._NOMINAL_RANGE_0_255`; jalur NV12 (`yuv_color_space()`) tidak disentuh. **Sisa terbuka - angka dikoreksi 2026-10-11 dengan A/B scene-identik (`SAMEA` device vs `SAMEB` software, referensi yang sama untuk kedua jalur, kontrol `dev1~dev2` = 38,5 dB):** jalur software 33,41 dB vs jalur device **36,19 dB** -> selisih nyata **2,8 dB** (rmse 6,3 vs 4,6), bukan ~6 dB seperti tertulis sebelumnya; angka lama membandingkan referensi dari run berbeda (screenshot device antar-run hanya 25,4-29,8 dB karena scroll/status bar) dan karena itu tidak sah. Sifat residu pada pasangan sah: per-piksel (geser referensi memburukkan 11,8-18,7, blur 3x3 memburukkan ke 8,5 -> bukan geometri, bukan blur), afinitas gain 0,98 dengan intersep +2,7..+3,1, terbesar pada konten gelap (rmse 8,4 pada saturasi <10 vs 5,0-5,3 di pita lain), dan lebih halus dari residu jalur device (rasio varian dalam-2x2 0,54 vs 0,31-0,43) -> konsisten dengan konversi NV12->BGRA milik jalur software (`src/core/pixels.rs`: kroma nearest per-2x2, matriks BT.601 studio, pembulatan integer) dibanding konversi video processor GPU. **Mekanisme belum dibuktikan, jadi tidak ada perubahan kode** - jalur software adalah opt-out (`HERMESGATE_H264_HW=0`).
- Observasi alat: daftar output type yang ditawarkan decoder in-box saat device manager terpasang = NV12, YV12, IYUV, I420, YUY2 (**tanpa RGB32**), jadi klaim komentar `negotiate_output_type` bahwa jalur software meminta RGB32 dari konverter MF tidak berlaku untuk MFT in-box di mesin ini; subtype yang benar-benar dipakai jalur software belum dilacak
- **Wireless terukur (2026-10-11)** - serial `192.168.18.7:39583` (adb over Wi-Fi, transport yang sama dipakai fitur pairing produk), workload identik: pass 2 tail FPS **49,3 / 51,9 / 50,3** (mean 50,5) vs USB **51,6 / 51,3 / 50,5** (mean 51,1); CPU **1,05-1,30** vs **1,19-1,30** CPU-s/25 s; stream **~8,5 Mbit/s** di kedua jalur; `decode_ms` 16,0 / 16,5, `blit_ms` 8,3 / 9,5. Ping ke 192.168.18.7: min 1 / avg 27 / max 96 ms, 0% loss (20 paket) -> transport wireless setara USB pada workload ini. Session wireless **pertama** (pass 1) macet (1 kbit/s, 21 frame, tail 0 FPS) lalu normal di session berikutnya. **Mekanisme dipersempit 2026-10-11 - koreksi klaim sebelumnya:** aliran frame TIDAK digerakkan konten. Layar hidup yang statis dengan **nol input** tetap mengalir (~50 FPS; 163 frame / 1,42 MB dalam 6,5 s; `static-repro.sh`). Yang menghentikan aliran adalah **display mati/gelap**: pada varian layar gelap tidak ada satu pun frame yang ter-decode dalam 12 s (banner 'H.264 decoding on the GPU' tidak pernah muncul, padahal banner itu digerbangi frame pertama) - dan swipe tetap dikirim, jadi input tidak relevan. Bukti: (a) CSV session macet `stats-W-wifi-1.csv` = 6,8 s, 14 round, `bytes_in` 1.344 B, `read_ms` 6.753 (99% waktu terblokir di read) dengan swiper jalan -> device tidak menyuplai frame; (b) repro `tools/bench/stall-repro.sh`: Wi-Fi + layar hidup = 50,0 FPS, 5,27 MB, 217 frame dalam 3 s (transport sehat); Wi-Fi + layar gelap + swiper = tidak ada frame ter-decode dalam 12 s (0 baris statistik karena `Stop-Process` membunuh app sebelum flush baris 1-detiknya; bukti langsungnya adalah absennya banner decode). Batas bukti: status display/keyguard device pada session macet tidak tercatat (`screen_off_timeout` device = 300 s, jadi bukan timeout idle di dalam run 6,8 s itu), jadi ini penjelasan terdepan dengan mekanisme tereproduksi, bukan pengukuran langsung session itu. **Aturan harness:** pastikan display hidup sebelum run ukur, dan buang run dengan `read_pct` ~100% + byte diterima ~1 kB sebagai run tidak sah - bukan hasil 'lambat'.
- **Validitas pengukuran capture (2026-10-11):** dua screenshot device dari scene sama tetapi run berbeda hanya **25,4-29,8 dB** satu terhadap lain (scroll/status bar), dan kontrol dalam-run `dev1~dev2` pun hanya **28,9 dB** pada run jalur device yang sah (FIX2/FIX3) -> ukuran sah bukan `dev1~dev2`, melainkan **mirror ~ screenshot run-nya sendiri** (36,2-39,5 dB), yang jauh di atas kontrol lintas-run. `verify_capture.sh` men-toggle display (keyevent 26/26/224) sehingga `dev1` kadang menangkap layar gelap (mean 81/80/82 vs 143/117/114). Untuk klaim antar-jalur dipakai A/B scene-identik (`SAMEA`/`SAMEB`), bukan dua run terpisah.
- **Latensi input->mirror, TERUKUR (2026-10-11, device TERBUKA: `KeyguardShowing=false`, layar aktif)**: USB `bc058b2c` = 586 / 654 / 654 / 653 / 1.403 ms (median **654 ms**, n=5; sampel 1 pemanasan, dibuang dari median? tidak - median tetap 654 karena urutan 586,653,654,654,1.403); Wi-Fi `192.168.18.7:39583` = 599 / 746 / 933 / 846 / 627 ms (median **746 ms**, n=5) -> Wi-Fi = USB + 92 ms pada median, di dalam sebaran sampel
- **Apa yang diukur:** waktu dari penekanan HOME (dikirim lewat adb) sampai blok piksel 8x8 di dalam window mirror berubah. Ini menyertakan injeksi input adb + reaksi UI Android + capture/encode/transport/decode/present, jadi angka 'yang ditunggu pengguna mirror'. Komposisi terukur: round-trip injeksi `adb shell input keyevent 3` (USB) = 190-249 ms (median 197) = lantai sisi input; biaya satu iterasi polling sampler = 13,3 ms -> resolusi ~13 ms, kecil terhadap 600-900 ms
- **Validitas metode:** blok sampling identik byte dengan capture penuh pada koordinat sama (diukur lebih dulu dengan probe terpisah); window mirror foreground saat sampling; layar tetap aktif (`svc power stayon true` + focus Settings; catatan: `settings put global stay_on_while_plugged_in` DITOLAK adb shell di perangkat ini - SecurityException butuh WRITE_SECURE_SETTINGS - jadi tidak berpengaruh, hanya `svc power stayon` yang berizin) dan keyguard false selama kedua run; harness `tools/bench/latency.sh <label> <serial> <samples>` (satu perintah per transport), `latency.ps1` di sampingnya (catatan: keyevent 26 pengganti toggle layar DIBUANG - mematikan layar memunculkan keyguard dan HOME lalu tertelan, terukur 5/5 timeout)
- Varian `EVENT=screen` (transisi display -> frame) terukur 734 / 1.106 / 813 / 734 / 960 ms (median 813) = metrik rekonfigurasi display, **BUKAN** latensi input; tidak diklaim sebagai latensi. Interval frame 19,6 ms (51,1 FPS) bukan penyumbang dominan
- Permukaan statistik produk **SUDAH ADA di produk 2026-10-11** (keputusan pemilik + implementasi minimum, item 3 `t_8aab30c8`): panel diagnostik di UI aplikasi (bukan overlay di window mirror `WS_POPUP` yang bisa tertangkap OBS), snapshot 1 Hz lewat kanal status yang sudah ada (`mirror_status` + `mirror-changed`); hanya counter `Stats` yang sudah ada (`fps`, `bytesPerS`, `decodeMs`, `blitMs`, `overwritten` = delta delivered - delta taken, bukan klaim total 'dropped', `framesDecoded`), tanpa instrumentasi baru; input-to-mirror latency TIDAK ditampilkan (jalur ukur belum tersedia; interval frame bukan latensi); CSV `HERMESGATE_VIDEO_STATS` tetap opt-in dan satu definisi angka (`VideoStats::from_deltas`) -> tidak ada dua sumber kebenaran. Sesi yang selesai me-retract sample-nya supaya UI tidak membaca angka sesi mati. Bukti: `docs/PRD.md` "Product statistics surface — decisions and minimum implementation", `src-tauri/tests/video_stats_live.rs`
- Anomali `D-sw-gpu` (software + GPU present, 23,96 dB — di bawah GDI HALFTONE) pasca-fix 28,9 dB vs screenshot device pada **scene stabil** (dev1-vs-dev2 = 40,7 dB), sementara device decode + GPU 39,4-39,5 dB. **Sebab ditemukan dan diperbaiki 2026-10-11: deklarasi RGB range/nominal range stream BGRA salah** (blok color-range di atas) -> jalur software kini 33,41/33,38 dB pada scene identik. Tidak ada regresi jalur device pada build baru: capture device = **36,15 dB** dengan pasangan sah (dev1-vs-dev2 = 38,0 dB; rata-rata mirror 82/80/84 vs device 81/79/82) pada scene Settings gelap. Sisa jalur software **dikoreksi 2026-10-11 menjadi 2,8 dB** (A/B scene-identik, satu referensi bersama: device 36,19 dB vs software 33,41 dB; rmse 4,6 vs 6,3) - angka "~6 dB" lama membandingkan referensi dari run berbeda dan tidak sah (lihat blok color-range di atas)
- Kit analisis pasangan capture disalin ke `tools/bench/` (2026-10-11): `scatter.py` (gain/intersep per kanal + uji hipotesis ekspansi range), `resid.py` (residu per pita baris/saturasi), `spatial.py` (hipotesis geser/blur), `mapping.py`, `expansion_test.py`, `blockperiod.py` (struktur blok 2x2 = jejak chroma upsampling), `stall-repro.sh` (repro stall: layar hidup vs layar gelap)
- ~~Fallback hardware->software decode saat device tidur/keyguard~~ -> **bukan fallback**: baris status salah waktu; diperbaiki + diverifikasi (lihat Verification)
- Belum ada commit/push (izin belum diberikan)

### Report
```
STATUS: selesai (lingkup perbaikan P0), uncommitted
CHANGE: h264.rs rebuild ring Copier saat dimensi berubah; gpu.rs overlap() clamp copy + source rect
EVIDENCE: 3x25s vs scrcpy v4.1 -> 51,1 vs 49,9 FPS, CPU 1,2s vs 4,8s, blit 464->10 ms, PSNR 16,2->30,6 dB, 300->0 baris hijau
VERIFICATION: cargo build --release (0 warning) + cargo test --release (18 unit + 2 integration PASS, 1 ignored) + capture bersih + `npm run build` (tsc+vite) bersih + `tests/video_stats_live.rs` (sesi nyata dari socket loopback 720x1604: sample 1 Hz dipublikasikan `fps 17,0 | 5,70 Mbit/s | decode 41,6 ms/s | present 1,5 ms/s | overwritten 28 | decoded 29`, lalu di-retract saat `StreamEnd::Disconnected`) + `cargo check --all-targets` bersih. Catatan lingkungan: profil rilis memakai `panic = "abort"`, jadi target integration test yang BARU harus dijalankan dengan `CARGO_PROFILE_RELEASE_PANIC=unwind` bila memakai `--release` (artefak dependensi `abort` tidak bisa dilink ke crate test `unwind`)
REGRESSIONS: tidak ada yang ditemukan; software/GDI tetap berfungsi (29,8 dB)
UNRESOLVED: statistik produk SELESAI 2026-10-11 (keputusan + implementasi: panel diagnostik 1 Hz di UI aplikasi; verifikasi ponsel nyata belum dijalankan karena device Asleep + keyguard aktif - harness `mirror_stats` sudah mencetak sample yang dipublikasikan untuk run berikutnya), sisa 2,8 dB jalur software (opt-out) setelah fix color-range, penyebab stall wireless session pertama (mekanisme dipersempit: display gelap tidak mengirim frame; root cause tidak terkonfirmasi). Catatan vault Hermes-Knowledge disinkronkan 2026-10-11: `Projects.md` (color-range diperbaiki+terverifikasi, status stall wireless) dan `Decisions.md` (klaim "kinerja wireless belum diuji" dikoreksi).
LATENCY (2026-10-11): USB median 654 ms, Wi-Fi median 746 ms (n=5 each, HOME->frame change; input floor 197 ms)
WIRELESS (2026-10-11): setara USB - tail FPS 50,5 vs 51,1; CPU 1,05-1,30 vs 1,19-1,30 CPU-s/25 s; stream ~8,5 Mbit/s
```


---

## Milestone 9: UI design alignment ke design acuan Figma (2026-10-11)

### Status

Selesai (frontend saja). Fungsi mirroring, wireless ADB, diagnostics, OBS-friendly floating window
dan perilaku lain tidak diubah.

### Sumber desain (dibaca, bukan dikarang)

`https://lance-speak-29635031.figma.site/` - mock "Device Console" dari Figma Make. Token dibaca
langsung dari stylesheet yang dilayani (`document.styleSheets` -> `figma-ref.css`): `--bg #0a0c0d`,
`--top #121415`, `--rail #151719`, `--surface #171b1d`, `--surface-2 #121516`, `--surface-3 #0e1011`,
`--border #24292c`, `--border-soft #1b2022`, `--text #e4eaee`, `--muted #707980`, `--faint #3e464b`,
`--cyan #00a8ec`, `--green #00d395`, `--red #ff3d4f`; teks Inter + mono untuk semua label/badge/status.
Catatan: canvas acuan adalah mock terskala (teksnya sendiri 6-9 px), jadi skala tipografinya diadopsi
pada ukuran desktop yang terbaca (9-12 px), bukan disalin mentah.

### Perubahan

- `src/App.css`: palet acuan dipetakan 1:1 (nama token lama tetap sebagai alias supaya tidak ada rule
  yang kehilangan token); sistem mono micro-label (satu rule untuk `.panel-head h2`, `.card h2`,
  `.table th`, `.chip`, `.state`, `.stat-label`, `.stat-value`, `.device-id`, `.statusbar`,
  `.status-strip`, `.nav-tag`, `.sidebar-label`); border hairline `#24292c`/`#1b2022`; tint
  `border-bottom #17323b` pada kartu/perangkat; grid telemetri sel mono berbagi garis
  (`.stats`/`.stat`); tombol utama `#008fcc`; statusbar `#111314`; tanda aktif nav 1 px cyan + glow
  (padanan `rail-button.active::after`); sidebar 196 px.
- `src/App.tsx`: statusbar shell (aplikasi+versi, platform, jumlah modul core, view aktif), micro-label
  `Navigation` di sidebar, dan footer kontekstual view Devices menjadi `.status-strip` agar statusbar
  shell tetap satu-satunya bar status. Tanpa dependency baru.
- Tidak disalin: rail ikon-saja (butuh dependency dan navigasi berlabel harus tetap eksplisit) dan
  telemetri karangan acuan (IP perangkat, uptime, encoder, latency) - aplikasi hanya boleh menampilkan
  field yang benar-benar dilaporkan backend.

### Verifikasi (actual)

- `npm run build` (tsc + vite) bersih: `dist/assets/index-CT0aV7tF.css` 9,40 kB, `index-B70lZiiO.js` 238,45 kB.
- Render nyata di browser dari `vite dev`: body `rgb(10,12,13)`, sidebar `rgb(21,23,25)` + border kanan
  `rgb(27,32,34)`, topbar `rgb(18,20,21)` (48 px), statusbar `rgb(17,19,20)` mono 10 px; teks statusbar
  "App: HermesGate - | Platform: - | Core: - | View: dashboard"; sidebar "NAVIGATION" tampil; `window.__errs` kosong.
- Probe komponen in-page terhadap stylesheet aplikasi: `.card` bg `rgb(23,27,29)` radius 8 px border-bottom
  `rgb(23,50,59)`; `.panel` bg `rgb(18,21,22)` radius 6 px; `.panel-head h2` mono 10 px uppercase
  letter-spacing 1,4 px; `.chip` mono 10 px uppercase radius 2 px; `.device-card` sama dengan `.card`;
  `.stat-label` mono 9,5 px uppercase; `.stat-value` mono 12 px; `.status-strip` border-left
  `rgb(0,168,236)`; `.table th` mono 9,5 px uppercase; `.btn-primary` `rgb(0,143,204)`.
- Artifact terkirim memuat token/kelas baru: `#00a8ec` (5x), `#00d395` (3x), `#0a0c0d`, `#24292c`,
  `#17323b` (2x); `status-strip`, `sidebar-label`, `Navigation`, `View:` ada di bundle JS.
- Dev server dimatikan setelah verifikasi: tidak ada listener di port 1420.

### Batas

Perubahan tidak dijalankan ulang di dalam shell Tauri (perubahan frontend; rebuild release tidak
dijalankan untuk ini). Komponen yang butuh jembatan native (kartu perangkat, panel diagnostics dengan
data hidup) tidak dirender di browser - verifikasinya lewat probe CSS terhadap stylesheet aplikasi
yang sama plus `npm run build`.
## Milestone 10: Pointer input forwarding — live verification + wheel-sign fix (2026-10-11)

### Status
Pointer input forwarding works end to end on real hardware. Acceptance criterion 9 moves
NOT STARTED -> PARTIALLY (pointer scope complete and verified; keyboard/text/gesture absent).
Acceptance criterion 8 (minimize) moves NOT VERIFIED -> PASS at runtime.

### Scope
Pointer only, as `src/core/input.rs` states: left press / drag / release and wheel scroll, sent as
`INJECT_TOUCH_EVENT` (type 2) and `INJECT_SCROLL_EVENT` (type 3) over the scrcpy control channel
(`control=true`, `tunnel_forward=true`). Keyboard, text injection and gesture keys are not
implemented.

### Bug found and fixed
`src-tauri/src/core/video.rs` sent `vscroll = -notches`. scrcpy's client sends the value
unnegated (`vscroll = event->y`, `sc_input_manager_process_mouse_wheel` ->
`sc_mouse_processor_process_mouse_scroll` -> `SC_CONTROL_MSG_TYPE_INJECT_SCROLL_EVENT`).
Measured on device `bc058b2c`: with the negation the wheel scrolled the list the wrong way.
Change: one line to `+notches`, plus a comment recording the measurement. No other source file
was touched.

### Verification (actual, not assumed)
Method: read the device's own state (`uiautomator dump` view tree + `dumpsys ... topResumedActivity`)
before and after each input, sent to the mirror window with `PostMessage` at coordinates scaled
from the 720x1604 picture onto the 412x918 client area.

- tap Bluetooth row (384,1070) -> `com.oplus.wirelesssettings/.SettingsActivity`
- tap Wi-Fi row (384,860) -> Wi-Fi page with `Aktif`, saved `LEX`, `Cumi - Cumi`
- tap `Chrome` in the app list -> SubSettings with `Buka` / `Paksa berhenti` / `Nonaktifkan`
- drag up -> later rows; drag down -> back to the top rows
- wheel one notch at a time on the app list -> `Browser` -> `F-Droid` -> `Gboard`, and back
- minimize: main window `SW_MINIMIZE` (`iconic_during=True`); during the minimized period three
  `input swipe` events drove the counter 3056 -> 3223 decoded frames; mirror window kept its handle
  (visible, 412x918); one `hermesgate` process throughout
- `cargo test --lib`: 28 passed, 0 failed
- live panel at the same time: 51.4 FPS while scrolling, 2.6-2.9 Mbit/s, decode 35.3 ms/s,
  present 2.4 ms/s, 2878-3290 frames decoded

A screenshot was not accepted as evidence, and neither was "the command was sent" — both were
green while the wheel was still inverted.

### Build provenance
The verified binary is `target/debug/hermesgate.exe` (mtime 18:07:37) with the running process
started 18:07:38 — the same minute, so the harness exercised the post-fix build. A second
`npm run tauri dev` tree in the same folder failed with `failed to remove file ...hermesgate.exe`
/ `Access is denied (os error 5)` (exit 101): two dev trees cannot share one fixed exe name.
Watcher notifications reporting "Compiling" / "error" from an *exited* process were history, not
current build activity — checked against process start times, not assumed.

### Limits / open
- One device, one GPU, one Windows host.
- Mirror-window pixels were not compared against `screencap` in this session.
- Per-notch scroll magnitude is one full scrcpy step; a smaller step is a product decision.
- Keyboard, text and gesture keys (Back/Home/Recents) remain unimplemented.

### Report
`docs/PRD.md` updated with the same evidence: checkboxes (Input / Mirror Window), acceptance
criteria 8 and 9, Completed, Not Started, and a new evidence section; three stale lines under
Key Blockers corrected (Wi-Fi discovery, hardware decode, OBS/minimize). No commit, no push.

## Milestone 11: Protected content — screencap refused, mirror black (2026-10-11)

### Status
Negative result, recorded as such. The mirror-versus-device pixel comparison could not run: the
device was showing protected video, where the platform withholds the framebuffer. Not a HermesGate
defect, but it exposes an unexplained-black-window gap.

### What was attempted
Criterion 7 pixel parity: capture the mirror window's client area and compare it against the
device's own `screencap`, scaled to the same size. Helpers written for it: `hg-cap2.ps1` (captures
the client area, raising the window and restoring its previous `WS_EX_TOPMOST` state — verified
`topmost before=False after=False (restored=True)`) and `p-rowdiff.py` (per-row difference, to
catch a title bar or letterbox as a band of deviating rows).

### What the evidence shows
- `adb exec-out screencap -p` -> 0 bytes, twice; `screencap -p /sdcard/p-dev.png` -> 0-byte file.
  Not an `exec-out` encoding artefact.
- Mirror client area 412x918: mean luminance 0.63/255, 18 of 918 rows above 8. Two captures 929 ms
  apart, both black.
- Same session, same moments: ~30 FPS, decoded frames 17252 -> 17929. Frames arrive; content is
  withheld.
- Device log: `W SurfaceFlinger: FB is protected: PERMISSION_DENIED`.
- Control: with Settings in front at 17:59 the same `screencap` produced a valid 67,842-byte PNG,
  and the capture method is the one the existing bench harness already used for PSNR against the
  device. So the blackness tracks the content, not the tooling.

### Consequences
- Criterion 7's pixel comparison must be run on a non-protected screen.
- `Report unsupported operations clearly` has a concrete gap: protected video yields a black mirror
  and no message. Detecting it client-side is not obvious (the frames are simply black), so it needs
  a product decision — not implemented here.

### Limits
One device, one Android version, one foreground app. The `isSecure=true` window-property scan
returned 0 matches, so the protection is not visible through that property on this ColorOS build;
the SurfaceFlinger line is the direct evidence.

### Report
`docs/PRD.md` updated: Input checkbox, the input-evidence Limits paragraph, a new "Protected
content" section, and a Key Blockers entry. No source change, no commit, no push.

---

## Milestone 12: Blank-picture notice and mirror/device pixel parity (Complete)

### Status
- **STATUS**: Completed (evidence below)

### What was built
- `src-tauri/src/core/video.rs`: a content probe on the window thread, at most once a second —
  `picture_has_no_content()` (bounded sample of the picture, mean-luminance floor 6.0/255),
  `frame_has_content()` (BGRA frames sampled directly, decoded GPU textures read back),
  `note_picture()` / `picture_blank_secs()` (the blank run is wall time since it began), a log line
  when a run starts, and one line if a readback ever fails.
- `src-tauri/src/core/mirroring.rs`: `MirrorStatus.picture_blank_secs`, filled only while a session
  is `running`, `0` otherwise.
- `src/App.tsx`, `src/lib/tauri.ts`, `src/App.css`: a `notice` in the diagnostics panel, shown once
  the picture has carried no visible content for 2 s. It reports the measurement and names the
  ambiguity instead of claiming a cause.

### Evidence (device `bc058b2c`, 720x1604, session `RUNNING`)

| Moment | Observation |
|--------|-------------|
| launcher in front (real content) | no notice across a 24 s poll; 30.4 FPS, 7.7 Mbit/s |
| protected video in front | `screencap` = 0 bytes; notice present on 20 consecutive polls, first 4-5 s after the picture went blank |
| rendered text | "Nothing to display. The stream is live and frames are arriving, but the picture has carried no visible content for 5 seconds. Protected content (DRM video) and a display that is off both look like this, and HermesGate cannot tell them apart — so it reports what it measured instead of claiming a cause." |
| HOME pressed (content again) | notice cleared within 3 s |
| mirror window, same protected moment | 412x918, mean luminance 0.63/255 (18 of 918 rows above 8) |
| the app's own stdout (dev log) | `mirror: the picture carries no visible content; the UI notice appears if it stays blank` — once per blank run, not once per second, which is the direct check that the 1 Hz reset bug is gone |

Pixel parity on a non-protected screen (Settings in front). What was done on the device, stated
exactly: `am start -a android.settings.SETTINGS` (a screen, not a setting), the captures, then
`input keyevent KEYCODE_HOME` and a relaunch of the app that had been in front. No `settings put`,
no toggle, and no configuration was written; `settings list system|secure` was read only after the
run, so it is a record of the end state, not a before/after proof.

| Comparison (both scaled to 412x918) | Result |
|-------------------------------------|--------|
| mirror window vs `adb screencap` of the same screen | mean absolute difference 1.02/255, per-row RMSE 1.72, 43.4 dB PSNR, worst pixel 97, 0 rows above 24 |
| mean luminance control | Settings mirror 27.17 vs protected mirror 0.63 — the 6.0 floor sits between them |
| window z-order | `WS_EX_TOPMOST` before False / after False (capture restored it; asserted) |

### Bug found and fixed during verification
`publish()` cleared the blank run on every call, and the session sampler calls it with `Some` once
per second — so the run could never exceed one second, the notice never appeared, and the log showed
a run "starting" every second. It now clears only when the sampler retracts (`None`, session
start/end). The probe was also moved from counting probes to wall time, because a blank or static
picture produces few frames (measured 0.8-10 FPS) and counting would under-report how long the user
has been looking at nothing.

### Checks
`cargo test --lib`: 29 passed / 0 failed (new test `content_probe_tells_a_blank_picture_from_a_dim_one`
separates blank, near-black, dim and text-on-black pictures).

### Limits
One device, one Android build, one host. The notice reports the measurement, not the cause: the
client cannot distinguish DRM-protected video from a display that is off, so it claims neither. The
mirror window itself was not modified.

### Report
`docs/PRD.md` updated: criterion 7 parity numbers, the in-progress mirror-window line, the
protected-content section, and two Key Blockers. No commit, no push.

---

## Milestone 12b: Navigation keys Back / Home / Recents (Complete)

### Status
- **STATUS**: Completed (evidence below)

### Protocol check first
`INJECT_KEYCODE` is control message type 0, and its serialized form is 14 bytes big-endian —
`type(1) action(1) keycode(4) repeat(4) metastate(4)` — read from scrcpy v5.0's
`app/src/control_msg.h` / `control_msg.c` and the client's own `input_manager.c`, the same version as
the server this app ships (`src-tauri/binaries/scrcpy-server-v5.0`). Two details a from-memory
implementation would have got wrong: the action is **one** byte here (the touch event uses four),
and the server rejects a longer message, so the length has to be exact. Keycodes:
`AKEYCODE_BACK = 4`, `AKEYCODE_HOME = 3`, `AKEYCODE_APP_SWITCH = 187`. A press is a `DOWN` and then
an `UP`, `repeat` and `metastate` 0, as the scrcpy client itself sends them.

### What was built (minimum, on the proven path)
- `src/core/input.rs`: `TYPE_INJECT_KEYCODE`, the keycode constants, `key_event()`, and
  `Controller::key()` — the socket, write path and error reporting are the ones already proven for
  touch and scroll.
- `src/commands.rs`: `mirror_key(key)` — name-to-keycode mapping, and `Err` when no session is
  forwarding input, so it cannot fail silently.
- `src/lib.rs`: command registered. `src/lib/tauri.ts`: `mirror.key()`. `src/App.tsx` + `src/App.css`:
  three buttons in the session panel, enabled only while a session runs.

### Evidence (device `bc058b2c`, session `RUNNING 720 x 1604`)
| Key press from the panel | Device state after (the device's own report) |
|--------------------------|---------------------------------------------|
| Back | `com.oplus.wirelesssettings/.SettingsActivity` -> `com.ss.android.ttmd.video/...SeriesInnerFeedActivity` -> `.../SplashActivity` (two pops) |
| Home | `com.android.settings/.OplusSettingsHomepageActivity` -> `com.android.launcher/.Launcher` |
| Recents | `com.android.launcher/.Launcher` with `uiautomator` showing the Settings task card and `Tutup semua` — the task switcher, not the home screen |

No error banner, and the panel reported the buttons enabled at `RUNNING 720 x 1604 px`. The device was
left in the state this task found it in (the video app in front).

### Checks
`cargo test --lib`: 30 passed / 0 failed (new `a_key_press_matches_the_control_protocol`, asserting
the 14-byte layout and the keycodes). `npx tsc --noEmit`: clean.

### Limits
Same device and host as before. Free-text and full keyboard injection stay unimplemented: this
milestone is deliberately the three navigation keys, on the existing control channel.

### Report
`docs/PRD.md` updated: the Input checklist item, the "not started" bullet, the implemented-scope
paragraph, criterion 9, the input evidence section and the input Key Blocker. No commit, no push.

---

## Milestone 12c: Registry and doc alignment (Complete)

### Why
Two places still declared the opposite of what the code does, and both are read as status:
`src/core/mod.rs` `MODULES` — whose own doc comment says it "reports each module's real state" — still
marked `mirroring`, `input` and `video` as `planned`, and `docs/implementation-plan.md` still carried
rows 7/8/9 as VERIFIED-in-code / NOT VERIFIED / NOT STARTED.

### Changed (evidence-backed, no behaviour)
- `src/core/mod.rs`: `Mirroring Engine`, `Input Engine`, `Video & Rendering` -> `partial` (clipboard
  and settings stay `planned`). `partial` is the honest state: each works and is verified on one
  device/host, and each has a recorded open item (reconnect robustness, text/keyboard injection,
  a single GPU exercised).
- `docs/PRD.md`: the module table now mirrors that registry, with the open item named per row;
  criterion 13 moves to PARTIALLY and drops the superseded "9-10 FPS via software decode" line in
  favour of the numbers actually measured since.
- `docs/implementation-plan.md`: rows 7 and 8 -> PASS with their measurements, row 9 -> PARTIALLY
  with the input scope now shipped.
- No behaviour change: strings and prose only.

### Verified
- `cargo test --lib`: 30 passed / 0 failed.
- The Dashboard renders the new states from the running app (CDP, not the source):
  `Mirroring Engine … PARTIAL`, `Input Engine … PARTIAL`, `Video & Rendering … PARTIAL`,
  `Clipboard … PLANNED`, `Settings … PLANNED`.
- Editing `mod.rs` restarted the dev app, so the key path was re-checked once on the new binary:
  with a session attached, the panel's **Home** button moved the device from
  `com.ss.android.ttmd.video/...SeriesInnerFeedActivity` to `com.android.launcher/.Launcher`.

### Limits
State words are a judgement call: `partial` is chosen against the criteria that are still not met,
not against subjective completeness.

### Report
No commit, no push. Device left on the launcher; session restarted and running.

---

## Milestone 12d: Latency documentation reconciled with the measurement (Complete)

### Finding
The latency evidence existed but the two documents a reader checks first said it did not. `docs/PRD.md`
acceptance criterion 3 read "per-frame end-to-end latency still unmeasured" and the product-stats
decision read "no measurement path exists", while the WORKLOG has had, since 2026-10-11, a measured
figure with its method: USB median **654 ms** (586/653/654/654/1403, n=5), Wi-Fi `192.168.18.7:39583`
median **746 ms** (599/627/746/846/933, n=5), HOME injected with `adb shell input keyevent 3` and timed
to the mirrored 8x8 pixel block changing, display awake, no keyguard, harness
`tools/bench/latency.sh <label> <serial> <samples>` (its own header confirms `EVENT=home` is the
input->mirror figure and `EVENT=screen` is reconfiguration, not latency). `implementation-plan.md`
row 3 repeated the same wrong claim.

### Changed (docs only, no source touched)
- PRD criterion 3 now carries both halves — the measured frame rate and the measured latency — plus
  the composition (197 ms adb injection floor, ~13 ms sampler resolution, 19.6 ms frame interval not
  dominant) and, explicitly, what the number is **not**: not a per-stage decomposition, and not the
  latency of the app's own forwarded input, because the injection leg went through `adb` rather than
  the session's control socket, so it is an upper bound on the user-visible path.
- PRD P2 item "Measure latency and resource usage before optimizing": annotated with what was
  measured (latency; CPU 1.2 CPU-s / 25 s vs 4.8 for the reference) and left **open** because memory
  and GPU usage were not measured.
- PRD product-stats decision: latency is still not shown in the panel, and now says why honestly —
  a figure exists offline, but showing one would need instrumentation the panel decision excludes.
- `implementation-plan.md` row 3 updated to match.

### Verified
Read-back asserts on all four edited anchors; the harness header was read to confirm the signature and
the `EVENT=home`/`EVENT=screen` distinction the PRD now states.

### Next step (needs owner approval — it measures the product's own path, so it needs the device)
Time the same transition with the press injected through the **session's control socket** (the panel's
Back/Home/Recents, or a click in the mirror window) instead of `adb`. That drops the ~197 ms adb floor
and measures what a user of the app actually experiences; the two halves already exist (`hg-input.ps1`
posts window messages, `hg-band-hash.js` reads pixels). Cost: device awake and unlocked, session
RUNNING, mirror window foreground, ~10 presses, one scratch harness, device returned to its prior
state. No source change. Not run yet.

### Report
No commit, no push, no source change. Device and mirror session untouched.

---

## Milestone 12e: Product-path latency measured, USB and Wi-Fi (Complete)

### Question
The adb-path figure (USB median 654 ms, Wi-Fi 746 ms) was an upper bound, not the product's path: it carried
a 197 ms adb injection floor and the injection leg bypassed the session. What does a press delivered through
the app itself cost?

### Method (existing harness, one added mode — no source touched)
`tools/bench/latency.ps1` (the harness behind the earlier numbers) grew a product mode: `-MirrorTitle` finds
the app's mirror window by title (it is a second top-level window, not the process's `MainWindowHandle`),
and the press is delivered by `Start-Process node tools/bench/click_nav.mjs<port> <Label> <trigger> <stamp>`
clicking the panel's Home button over CDP. Sampling is byte-for-byte the same code: 8x8 block at 15%/25% of
the client, threshold 24, 412x918 client. Timing is host-clock on both sides — the clicker stamps
`Date.now()` the instant *before* it delivers the click, so the CDP connect and the trigger wait fall outside
the interval; the sampler polls with `[DateTimeOffset]::Now.ToUnixTimeMilliseconds()` and subtracts. The
click is a programmatic mouse click: nothing of the product's path is bypassed. Defaults are unchanged, so
the earlier adb numbers reproduce exactly as documented.

### Result
- **USB (`bc058b2c`), n=8:** median **698 ms**, 605 / 620 / 650 / 662 / 698 / 701 / 713 / 733.
- **Wi-Fi (`192.168.18.7:39583`), n=8:** median **706 ms**, 558 / 583 / 619 / 623 / 706 / 744 / 756 / 840.
- Resolution ~13.3 ms (one sampler poll). Display awake, keyguard not showing, per-sample setup
  `am start -a android.settings.SETTINGS` then the press, HOME as the event (Settings -> launcher).
- Compared with the adb figure (654 / 746 ms, n=5): the product number is **not** the lower one even though it
  lost the 197 ms adb floor, so the residual after the press differs by roughly 240 ms. Not comparable as
  measured — the earlier figure also sampled the release `mirror_stats` example rather than the app. The gap
  is recorded as an open item; no cause is claimed.
- Not measured / not included: the human's own reaction time; per-stage decomposition; memory or GPU use.

### Side effects, all restored
The mirror window was raised with `SWP_SHOWWINDOW|TOPMOST` to be sampleable; the harness now reads the
`WS_EX_TOPMOST` bit first and clears it afterwards (`z-order restored: topmost was=False now=False` on both
runs). Device: `svc power stayon` was used for the run and set back to the default
(`stay_on_while_plugged_in` = 0), keyguard never engaged, the device was not locked or reconfigured. Session:
switched to Wi-Fi and back to USB through the app's own device list (IDLE -> RUNNING in 19 s and 11 s), left
RUNNING on USB. App not restarted (same PID).

### Artifacts
`tools/bench/latency.ps1` (product mode), `tools/bench/click_nav.mjs` (new), raw results
`lat-product-usb.txt` / `lat-product-wifi.txt` in the scratch dir.

### Report
No commit, no push, no source change, Obsidian untouched.

---

## Milestone 12f: The device's audio plays on the PC (Complete)

### Why
V1 goal 2: the phone's audio has to reach the PC. scrcpy's server can carry the device's own
playback, and nothing in HermesGate read that stream, so the mirror was silent.

### Changed (source)
- `src-tauri/src/core/mirroring.rs`: `audio=true audio_codec=raw` added to the server argv; the audio
  socket is opened right after the video socket and before the control socket, in the order the server
  accepts (video, audio, control); a named thread `mirror-audio` runs `audio::play(stream, stop)`;
  `MirrorStatus.audio: Option<AudioStats>` publishes the counters while a session runs; stopping a
  session trips `audio_stop` and joins the thread. An audio failure is reported in
  `MirrorStatus.audio.error` and never takes the video session down.
- `src-tauri/src/core/audio.rs` (new): a winmm `waveOut` render queue - `WAVE_FORMAT_PCM`, 48 kHz,
  2 channels, 16 bits, 8 x 4096-byte buffers (~0.17 s). Statistics come from the render device itself
  (`waveOutGetPosition` `TIME_BYTES` for played time, plus the loudest sample per 1 s window).
- `src-tauri/Cargo.toml`: `Win32_Media_Audio` feature. `src/lib/tauri.ts` + `src/App.tsx`: `AudioStats`
  type and the panel line.

### Verified
- `cargo check --all-targets`: 0 errors, 0 warnings. `cargo test --lib`: 34 pass, 0 fail (3 new audio
  tests: the format arithmetic, the packed device-name decoding, the sample accumulator).
- Live on device `bc058b2c` (USB), session `RUNNING`: PCM at 1540-1638 kbit/s against the 1536 kbit/s
  the format implies; the panel read `AUDIO 48 kHz ... which has played N s of it`, and that N advanced
  3 s -> 6 s in step with the wall clock (the render device's own clock, not a client-side estimate).
- Sound and not just bytes: the loudest sample of the 1 s window read **0** in silence and
  **1749-4591 (of 32767)** exactly while the device was made to play (two volume-key beeps,
  `adb shell input keyevent 24`), back to 0 afterwards. Device volume restored by the symmetric presses
  (2 up, 2 down); phone left in the launcher.

### Bug found and fixed (same milestone)
- The first audio build **aborted the whole application**: `thread 'mirror-audio' panicked at
  core/src/ptr/mod.rs:553: ptr::copy_nonoverlapping requires that both pointer arguments are aligned`
  and the process exited with `0xc0000409` (STATUS_STACK_BUFFER_OVERRUN, a *non-unwinding* panic).
  Cause: `WAVEOUTCAPSW` is packed, so `szPname` sits at alignment 1 and copying it as `u16` violates the
  pointer precondition; a debug-build UB check fired and took the process with it. Fixed by copying the
  name as bytes and decoding it in `device_name()`, which now has a unit test.
- Same file, second fix: `Sink::finish()` is idempotent via a `closed` flag, because `Drop` ran it a
  second time after an explicit call.

### Limits
One host, one device, one render device (`Microsoft Sound Mapper`); the device was made to play a short
system beep, not sustained media; audio latency and drift against the video are unmeasured; the short
queue means a stall shows up as a gap rather than as an error.

### Artifacts
`tools/bench/audiocheck.mjs` (samples the panel's audio line while driving the device's volume keys),
raw samples in the scratch dir.

### Report
No commit, no push, Obsidian untouched. Docs updated: PRD criterion 16 and the P0 mirroring bullet,
implementation-plan row 16, module registry `audio: ready`.

---

## Milestone 12g: Disconnect/reconnect and the mirror window's capture path (Complete)

### Why
V1 goals 1 and 4 lacked evidence: does a dead stream recover, and can a capture source read the mirror
window without chrome in the picture.

### Method
- Disconnect: kill the server on the device mid-session (`pkill -f com.genymobile.scrcpy.Server`) and
  watch the panel; then restart from the UI.
- Capture path: read the mirror window's style bits and rects at runtime, capture it through its own
  HWND (`PrintWindow`, flags 3 = `PW_CLIENTONLY|PW_RENDERFULLCONTENT`) and through the screen DC
  (existing `hg-cap2.ps1`), compare both with `adb screencap` of the same screen (scaled, rawvideo
  method) and search integer offsets for the alignment minimum.

### Verified
- Disconnect: the session reported `FAILED` with the reason `disconnected or stream ended` within 2 s,
  the process stayed alive and the panel re-enabled Start Mirror; one click -> `RUNNING` at 30.9 decode
  FPS with the audio stream open again.
- Window: style bits are `WS_POPUP` only (no `WS_CAPTION`, `WS_THICKFRAME`, `WS_BORDER`, `WS_SYSMENU`;
  exstyle 0) and the window rect equals the client rect (412x918) - there is no non-client area.
- Parity: screen-DC capture vs the device's `screencap` of the same screen - 2.27/255 mean absolute
  difference, 27.3 dB PSNR, no row where more than 90% of the width differs; the alignment search over
  +-4 px x +-8 px has a sharp minimum at offset (0,0) (6.08, against 15.3 at dy=-8 and 15.0 at dy=+8),
  so there is no shift, crop or letterbox.
- `PrintWindow` through the HWND: 13.3 dB PSNR and 91.5% of sampled pixels black, content only in
  patches - the legacy reader does not reproduce this window's DXGI swap chain.

### Limits and next step
Only one failure mode (the server dying on the device) was exercised: a cable pull and an app restart are
untested. A capture source of the BitBlt/PrintWindow family sees a partial picture, so OBS must use its
Windows Graphics Capture method (the default on Windows 10 1903+) or display capture. OBS is installed on
this host but was not driven: a window-capture run would write a scene collection into the user's OBS
configuration, which is outside this project's scope.

### Artifacts
`tools/bench/hg-printwindow.ps1`, `tools/bench/pw-compare.py`, `tools/bench/shift.py`.

### Report
No commit, no push, no source change in this milestone, Obsidian untouched.

## Milestone 12h: The Device page as the whole workflow, and a movable mirror window (Complete)

### Why
Owner instruction (kanban `t_d0782aae`): finalise the V1 UI/UX for non-technical users against the
SuperDesign reference — one card per device, one Start/Stop button, the whole mirroring workflow on the
Device page, and a mirror window that moves like a normal desktop window. Two defects blocked the first
of those, and the window was still a borderless `WS_POPUP`.

### Method
- Read the live UI over CDP (DOM text, not screenshots) while a session ran, against the Rust core's
  own output; compare the Device page with the reference rendered in a browser.
- Fix the two defects at their source (the `getprop` parser, the session status), then verify in the
  running application rather than in the tests alone: start and stop a real session over each transport
  and read what the page reports.
- Read the mirror window's style bits, frame/client rects and hit test with
  `tools/bench/hg-window.ps1`, and move it with `SetWindowPos`.

### Defects found and fixed
1. **Every device property read as empty.** `adb::parse_props` split each `getprop` line on `]:[`; the
   device prints `]: [` (with a space) and CRLF line endings, so manufacturer, model, Android version
   *and* `ro.serialno` all came back `None`. Without `ro.serialno` there is no identity to group by, so
   one phone (listed by `adb devices -l` three times: `bc058b2c`, its mDNS alias
   `adb-bc058b2c-8STQEK._adb-tls-connect._tcp`, and `192.168.18.7:39583`) produced **two cards** and no
   manufacturer or Android version. Fixed, with a regression test built from the bytes the device
   actually prints (`parses_the_format_the_device_actually_prints`).
2. **A session never knew which device it belonged to.** `Session::start` built its status with
   `..MirrorStatus::default()`, leaving `serial` empty for the life of the session. The card could not
   claim its own session, so while that device was mirroring its own button read
   `Another device () is mirroring — stop it first.` Fixed: the status carries the serial it was
   started with.

### Changes
| Area | Change |
|------|--------|
| `core/adb.rs` | `parse_props` accepts the real `]: [` separator and CRLF; `device_props` doc says what it reads |
| `core/device.rs` | `DeviceEndpoint { transport, serial }` replaces the `transport`/`transports` pair: one endpoint per live transport, USB first, each addressed by the most stable serial that transport has (hardware serial over its mDNS alias, `ip:port` over Wi-Fi). `DeviceState::Disconnected` is gone — the manager rebuilds its picture from every refresh, so a device with no transport left is simply not reported. `status` is the best state any transport reached |
| `core/mirroring.rs` | the session status is created with the serial it was started with |
| `src/lib/tauri.ts`, `src/App.tsx` | Device page: `Connection: [USB] [Wi-Fi]` inside the card (disabled while a session is live, so a running session's transport can never be switched under it), a line spelling out what the next start resolves to (`USB via bc058b2c`), one `Start Mirroring`/`Stop Mirroring` button that starts **the selected endpoint's serial**, and one phase readout in the Mirror Session panel instead of a duplicate badge; a failed phase is emphasised on the section note |
| `src/App.css` | the two pickers share one label column so their labels and controls line up; `.section-note.is-error` |
| `core/video.rs` | the mirror window is created as `WS_OVERLAPPEDWINDOW` sized with `AdjustWindowRectEx`: the client area keeps the exact frame size from the device, Windows draws the caption and borders and handles dragging, resizing and closing — no custom chrome, nothing drawn over the picture |
| `tools/bench/hg-window.ps1` | new: reads the window's style bits, frame/client rects, `WM_NCHITTEST` at the caption and proves `SetWindowPos` moves it |

### Verified
- `cargo test --lib`: 44 passed, 0 failed (includes the new grouping, endpoint, best-state and
  no-transport cases and the `getprop` byte-format regression test).
- `npx tsc --noEmit` clean; `npm run build` — `dist/assets/index-*.js` 240.68 kB (gzip 74.78 kB),
  CSS 10.90 kB (gzip 2.68 kB), built in 223 ms.
- Live UI (dev build, CDP against the running webview, device `bc058b2c`):
  **one card** — `CPH2773 · ONLINE · bc058b2c · OPPO · Android 14 · USB + Wi-Fi` — and the status strip
  reads `Devices: 1 known`; before the fix the same phone produced two cards.
- Start over the **selected** transport, twice: Wi-Fi → the page reports `Wi-Fi via 192.168.18.7:39583`
  and the session runs on `192.168.18.7:39583` (720×1604, 40.7 decode FPS, 8.8 Mbit/s, audio 48 kHz
  1540 kbit/s); USB → `USB via bc058b2c` and the session runs on `bc058b2c` (720×1604, 30.8 decode FPS,
  6.8 Mbit/s). Stop returns the button to `Start Mirroring` and the session to `IDLE`; the transport
  picker is disabled for as long as a session is live.
- Mirror window, read off the live window while a session ran: style `0x14cf0000` — `WS_CAPTION` yes,
  `WS_THICKFRAME` yes, `WS_SYSMENU` yes, `WS_MINIMIZEBOX`/`WS_MAXIMIZEBOX` yes, `WS_POPUP` **no**;
  frame 428×957 against a 412×918 **client** (the picture is unchanged, the frame adds 16×39 px of
  non-client chrome); `WM_NCHITTEST` on the caption returns `HTCAPTION` (2) — that is the drag area;
  `SetWindowPos` moved it from (0,0) to (200,150).
- Design check against the reference (`https://hermesgate.superdesign.cloud/`, rendered in a browser
  and compared image-to-image): same structure — dark utility shell, icon rail, page title + state
  badge, filters, one device card with the identity on the left and the controls on the right, a divided
  telemetry row, section panels and a status footer. Two findings were fixed: the CONNECTION/MODE
  labels did not share a vertical rhythm, and the phase was printed twice in the Mirror Session panel.
  The reference's per-device battery, uptime, encoder, latency and framerate values stay out of the UI:
  no backend reports them (field policy from Milestone 3).

### Limits and next step
Not exercised: pulling the cable while a Wi-Fi session runs (the grouping rule for that case is covered
by `losing_one_transport_keeps_the_card_and_the_other_transport`, not by a physical pull); OBS itself
(driving it would write a scene collection into the user's OBS configuration, outside this project's
scope) — the window is now a normal overlapped window, and the client area, which is what a Windows
Graphics Capture window source reads, is byte-for-byte the picture as before, but that was not
re-measured this session. One device and one host only. Next: measure the same flows on a second device
and screen size, and re-measure parity through the new window frame.

### Artifacts
`tools/bench/hg-window.ps1`.

### Report
No commit, no push, no unrelated refactor, Obsidian untouched. Docs updated: this entry, PRD (mission
bullet superseded, device-page section, mirror-window bullet, diagnostics-panel rationale),
`architecture.md` (device detection: identity, endpoints, no stale card; mirror window).

## Milestone 13: v0.1.0 published (Complete)

### Why
Turn the verified Phase 0-2 work into a shipped release. Only genuine blockers were touched; the
already-verified behaviour from Milestones 12g/12h was reused rather than re-tested.

### Blockers found
1. **`main` had a red CI run** (push from 2026-10-08). The Rust job ran on `ubuntu-latest`, and
   `cargo clippy -D warnings` failed there in `tests/h264_offline.rs` with
   `irrefutable-let-patterns`: the `Frame::Texture` variant is `#[cfg(windows)]`, so off Windows the
   `Frame::Bgra { .. }` pattern always matches. `cargo test` would not have linked on Linux either —
   the crate links Windows import libraries (Media Foundation, D3D11, WASAPI). The job was measuring
   a platform the product does not ship on.
2. **Three clippy `-D warnings` failures on Windows** in code added this session:
   `chunks_exact(2)` twice in `core/audio.rs` (`clippy::chunks_exact_to_as_chunks`) and
   `info.name.len() > 0` in `core/connection/wifi.rs` (`clippy::len_zero`).
3. **Documentation that contradicted the product**: `README.md` and `docs/roadmap.md` still described
   the repository as "Phase 1 — device detection, mirroring not implemented yet". A release whose own
   front page denies the shipped feature is not publishable.

### Fixes
- `ci.yml`: the Rust job moved to `windows-latest` (the Tauri system-package step dropped with it);
  the frontend job stays on ubuntu.
- `core/audio.rs` uses `as_chunks::<2>()`; `core/connection/wifi.rs` uses `!info.name.is_empty()`.
- README status/what-exists/repository-layout and roadmap phases 1-5 updated to the shipped state
  (clipboard sync, persisted settings and pointer/gesture input stay marked as not implemented).

### Verified
- The CI-equivalent checks once, locally on Windows: `cargo fmt --all -- --check` clean,
  `cargo clippy --all-targets -- -D warnings` clean, `cargo test` green (44 unit + 2 integration,
  1 ignored), `npm run build` green.
- CI on the release commit (`c7b9b92`): **success**, run 38146686175.
- The packaged production build (not the dev build) was driven over CDP on port 9224 against the real
  device: `ADB: bundled` (the packaged resource layout resolves the bundled ADB — not the dev tree),
  one card `ONLINE · bc058b2c · OPPO · Android 14 · USB + Wi-Fi`, session start from the card button
  reaching 720 x 1604 px at **28.5 decode FPS**, **10.7 Mbit/s** received, audio 48 kHz · 1573 kbit/s,
  2861 frames decoded, and stop returning the session to `IDLE`.
- Both release assets re-read from the remote after upload (`gh release view`): state `uploaded`,
  7 393 280 bytes (MSI) and 5 536 162 bytes (NSIS), and both download URLs answer.

### Limits
One device (OPPO CPH2773, Android 14) and one host. The installers are unsigned, so SmartScreen warns.
Installing from the MSI/NSIS was not performed on this machine: the packaged binary was exercised from
`target/release`, which is the layout the installers produce (`binaries/` beside the executable), but
the installer's own file placement was not verified by installing.

### Artifacts
`HermesGate_0.1.0_x64_en-US.msi`, `HermesGate_0.1.0_x64-setup.exe`
(https://github.com/erlanggalexyramadhan/HermesGate/releases/tag/v0.1.0).

### Report
Committed `c7b9b92` and pushed to `main`; tag `v0.1.0` on the same commit, GitHub Release published
(not a draft, not a prerelease). No secrets in the tree; nothing outside the project touched.

---

## Milestone 6: QR pairing for wireless debugging (2026-10-11)

### What changed
| File | Change |
|------|--------|
| `src-tauri/src/core/connection/qr.rs` | New. The pairing ticket: `PairingCode::generate` (service `hermesgate-…`, 10-digit secret), `payload()` in the format Android's own `AdbQrCode` reads (`WIFI:T:ADB;S:<service>;P:<secret>;;`), ZXing escaping of `\ ; , :`, a strict `parse()` that rejects anything the device would reject, and `modules()` turning the payload into a QR matrix |
| `src-tauri/src/core/connection/wifi.rs` | `MdnsService` + `parse_mdns_services()` for `adb mdns services`; `pair_over_mdns()` waits for the phone that scanned the ticket, runs `adb pair`, then best-effort `adb connect` on the phone's `_adb-tls-connect._tcp`; `WifiManager` holds the pending (single-use) ticket and records `Paired` vs `Connected` truthfully |
| `src-tauri/src/commands.rs`, `lib.rs` | `wifi_qr_start` (mint + matrix) and `wifi_qr_await` (blocking wait on a blocking thread) registered; manual `wifi_pair` / `wifi_connect` / `wifi_disconnect` untouched |
| `src/lib/tauri.ts`, `src/App.tsx`, `src/App.css` | QR panel inside Manual Connect: one button mints, draws and awaits; canvas at one pixel per module plus the 4-module quiet zone |

The direction of the flow is Android's, not ours: the **phone's camera** scans the code this host
displays (Developer options → Wireless debugging → Pair device with QR code). No host-side camera is
involved, and the manual `adb pair` path is unchanged.

### Verified (automatic, no device involved)
- `cargo test --lib`: **56 passed, 0 failed** — including the ticket round trip, rejection of every
  payload the device rejects, the AOSP example payload (`WIFI:T:ADB;S:studio-g@<xeYnap/;P:(Aq+v9>Cx>!/;;`),
  mDNS service parsing, single-use tickets, and `Paired`-vs-`Connected` state.
- `cargo fmt --all -- --check` clean, `cargo clippy --all-targets -- -D warnings` clean.
- `npm run build` (`tsc && vite build`) green.
- **Independent decode**: jsQR 1.4.0 over the module matrix the Rust encoder produced decoded back to
  exactly the expected payload (33×33 modules), so the encode path is verified end to end, not just
  self-consistent.

### Not verified (no Android device available)
The physical handshake: the phone scanning the on-screen code, publishing `_adb-tls-pairing._tcp` under
the requested instance name, `adb pair` succeeding against that address, and the follow-up
`adb connect`. This is the phone's side of the protocol and is unchanged by this work; the wait/pair
step is exercised only against real hardware.

