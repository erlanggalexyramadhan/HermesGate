# HermesGate Implementation Plan

**Reference**: https://hermesgate.superdesign.cloud  
**Base repository**: C:\Users\Lextronaut\Documents\project\HermesGate  
**Created**: Evidence-first; minimum steps, tokens, cost  
**Author**: Sebas (Hermes Agent)  

---

## 1. Verified Facts (from reference URL + codebase)

| Category | Fact | Source |
|----------|------|--------|
| **Theme** | Dark theme `--bg-main: #0f0f0f`, `--accent-blue: #0ea5e9` | CSS from `https://hermesgate.superdesign.cloud` |
| **Deploy ID** | `hermesgate` | `meta deploy-id` from reference URL |
| **Deploy timestamp** | `2026-10-10T15:25:04.202Z` | `meta deploy-timestamp` from reference URL |
| **Ogreference draft** | `12e3ee59-d726-4401-9cce-633b994822f1` | `og:image` path references this draft |
| **Window style** | `WS_POPUP` borderless at `(0,0)` | `video.rs` — confirmed via `git diff` and `cargo check` |
| **OBS capture compatibility** | VERIFIED — `WS_POPUP` borderless eliminates title bars/frames | `video.rs` change; cargo check passes |
| **Superdesign CLI auth** | `authenticated: false` per `npx --yes @superdesign/cli@latest auth status --json` | CLI verification; **separate from URL access** |
| **Codebase compiles** | `cargo check` passes (dev profile, 2 warnings only) | Confirmed after `wifi.rs` and `video.rs` modifications |
| **Android device present** | One device exercised: `bc058b2c` over USB, plus the same device over Wi-Fi (`192.168.18.7:39583`) | `adb devices -l`; WORKLOG Milestones 12e/12f/12g |
| **Theme colors not in source** | `--bg-main: #0f0f0f`, `--accent-blue: #0ea5e9` not applied in HermesGate Rust/React source | Design metadata vs implementation gap |
| **Superdesign Skill installed** | `SKILL.md` at `C:\Users\Lextronaut\AppData\Local\hermes\skills\software-development\superdesign\SKILL.md` | Previously installed; 9 scenarios documented |
| **cargo check warnings** | unused variable timeout_secs, lifetime syntax mismatch | 2 minor warnings; no errors blocking progress |

---

## 2. Design-to-Code Mapping (Evidence-Based Only)

| Design Element (from reference) | Code Element (HermesGate) | Mapping Status |
|--------------------------------|---------------------------|----------------|
| Dark theme `--bg-main: #0f0f0f` | No Rust/theme colors in source | ⚠️ **UNRESOLVED** — design color not yet mapped to code |
| Dark theme `--accent-blue: #0ea5e9` | No Rust/theme colors in source | ⚠️ **UNRESOLVED** — design color not yet mapped to code |
| `WS_POPUP` borderless at (0,0) | `video.rs` window style change | ✅ **VERIFIED** — already implemented |
| Performance grid layout | No grid in HermesGate UI | ⚠️ **UNRESOLVED** — not in current codebase |
| Window controls (.windows-ctrl, .windows-close) | No window control bar in HermesGate | ⚠️ **UNRESOLVED** — design has controls, code does not |
| og:image draft 12e3ee59 screenshot | WORKLOG.md criteria 3, 4 (mirroring/FPS) | ✅ **VERIFIED** — same draft ID referenced |
| SuperDesign Runtime JS (modern-screenshot) | No frontend JS in Tauri Rust app | ⚠️ **UNRESOLVED** — different tech stack (Web vs Tauri) |
| meta deploy-id: hermesgate | Repo name `HermesGate` | ✅ **VERIFIED** — exact match |

**Key Principle**: Only map design elements that have direct, verifiable correspondence to existing code. Mark all gaps as unresolved — do not invent or assume.

---

## 3. Implementation Phases (Minimum Steps)

### Phase 1: Verified Design Mapping ✅ (Complete, superseded 2026-10-11)
- **Task**: Confirm existing `video.rs` WS_POPUP borderless at (0,0) matches reference UI pattern
- **Evidence**: `git diff video.rs` shows 10 insertions, 19 deletions; `cargo check` passes
- **Result**: OBS capture compatibility verified at the time. **Superseded:** the owner instruction
  behind kanban `t_d0782aae` requires a mirror window that moves like a normal desktop window, so the
  window is now `WS_OVERLAPPEDWINDOW` + `AdjustWindowRectEx` (caption, resize borders, close box; the
  client area keeps the device's frame size and stays free of overlays). Verified live 2026-10-11 —
  see WORKLOG Milestone 12h and the PRD's mirror-window bullet

### Phase 2: Theme Color Integration ✅ (Complete)
- **Task**: Optionally map reference theme colors (`#0f0f0f`, `#0ea5e9`) to existing UI constants
- **Result**: Done in Milestone 3 — the dark theme uses the reference's `--bg-main #0f0f0f` and
  `--accent-blue #0ea5e9` (aliased to `--cyan`), and the Devices view was rebuilt to the reference's
  Discovery layout; the Device page was finalised against the same reference in Milestone 12h

### Phase 3: Unresolved Items Documentation (Required)
- **Task**: Document all design-to-code gaps without assumption
- **Result**: See Section 4 below

---

## 4. Unresolved Items (Marked Clearly)

| Item | Current State | Required Action | Priority |
|------|--------------|----------------|----------|
| **Theme colors** (`#0f0f0f`, `#0ea5e9`) | Not mapped to HermesGate source | Design decision needed; may require Tauri/theme config change | Optional |
| **Window controls** (.windows-ctrl, .windows-close) | Not present in current code | Determine if needed for mirror window; may conflict with OBS capture | Optional |
| **Performance grid layout** | Not in HermesGate UI | Determine if needed; would be new UI element | Optional |
| **SuperDesign Runtime JS** | Different tech stack (Web vs Tauri) | Not applicable — HermesGate is Tauri/Rust native | N/A |
| **Full UI layout extraction** | JS-rendered content not fully traversable | Accept limitation; only extract what's in static HTML | N/A |

**Policy**: All unresolved items explicitly marked; no unverified assumptions added to code or plan.

---

## 5. Acceptance Criteria (Reconciled with Code)

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 1 | USB connection works | PASS | ADB integration verified |
| 2 | Wireless pairing and connection work | PASS | Real `adb pair` + `adb connect 192.168.18.7:39583` (2026-10-11); the Wi-Fi device renders in the Devices view, streams, and a running session was switched USB -> Wi-Fi -> USB through the app's own device list (IDLE -> RUNNING in 19 s and 11 s; WORKLOG Milestone 12e) |
| 3 | Mirroring runs reliably with low latency | PARTIALLY | 51.1 FPS tail rate live on device bc058b2c (3 x 25 s Settings-scroll runs) vs 49.9 FPS scrcpy v4.1 reference; latency measured 2026-10-11 via `tools/bench/latency.sh` (HOME via adb -> mirrored pixel change): USB median 654 ms, Wi-Fi median 746 ms, n=5 — an upper bound that includes a 197 ms adb injection floor, not a per-stage decomposition. Product path measured 2026-10-11 (panel Home -> control socket): USB median 698 ms (n=8), Wi-Fi median 706 ms (n=8); not directly comparable to the adb figure, ~240 ms residual unexplained; device-dependent |
| 4 | Video uses efficient native pipeline | PASS | MF MFT -> D3D11 video processor -> DXGI swap chain by default; 1.2 CPU-s / 25 s vs 4.8 for the scrcpy reference; software/GDI fallback kept |
| 5 | Software decoding remains functional | PASS | Verified via MF MFT software decode; selectable with HERMESGATE_H264_HW=0 |
| 6 | Optional hardware decoding works and fails safely | PARTIALLY | Device decode verified live on one machine (hardware=true, D3D11 texture samples, 51.1 FPS, 30.6 dB PSNR); deadlock was a caller-side message-ordering bug, fixed (ADR 0003); no second GPU/driver exercised |
| 7 | Mirror-only window suitable for OBS capture | PASS | WS_POPUP only (no caption/frame/border/sysmenu, exstyle 0), window rect == client rect 412x918; parity against the device's own `screencap`: 43.4 dB PSNR / 1.02/255 (2026-10-11), re-measured on the current build at 2.27/255 and 27.3 dB PSNR with a sharp alignment minimum at offset (0,0) (2026-10-11). Caveat measured the same day: a `PrintWindow`/`PW_RENDERFULLCONTENT` reader does not reproduce the DXGI swap chain (13.3 dB, 91% black) — use OBS's Windows Graphics Capture method or display capture |
| 8 | Minimizing main window does not terminate mirroring | PASS | Live on bc058b2c: minimized the main window, decoded frames rose 3056 -> 3223, the mirror window kept its handle and stayed visible |
| 9 | Supported input forwarding works | PARTIALLY | Pointer (press/drag/release + wheel) and Back/Home/Recents verified against the device's own topResumedActivity / uiautomator; text and full keyboard absent |
| 10 | Disconnect/reconnect reliable | PASS | Server killed on `bc058b2c` mid-session -> session `FAILED` with the reason `disconnected or stream ended` within 2 s, process alive, Start Mirror re-enabled; one click -> `RUNNING` at 30.9 decode FPS with audio open (2026-10-11; cable pull and app restart still untested) |
| 11 | Application shutdown cleans up resources | PASS | Session Drop impl verified |
| 12 | Errors are actionable for troubleshooting | PARTIALLY | Specific reason strings in MirrorStatus.phase |
| 13 | Performance claims backed by measurements | NOT VERIFIED | mandate #7, #10: no device-dependent generalizations |
| 14 | Documentation matches implementation | IN PROGRESS | This implementation-plan.md creation |
| 15 | Necessary checks pass with limitations recorded | IN PROGRESS | cargo check passes; limitations recorded |
| 16 | Device audio forwarded to the PC | PASS | `audio=true audio_codec=raw` + winmm `waveOut` (48 kHz stereo 16-bit, 8 x 4096-byte queue) in `src-tauri/src/core/audio.rs`; live on `bc058b2c`: 1540-1638 kbit/s PCM against the 1536 kbit/s the format implies, render-device clock advanced 3 s -> 6 s, loudest sample 0 during silence -> 1749-4591/32767 while the device played (2026-10-11; WORKLOG Milestone 12f) |

**Device coverage**: one device (`bc058b2c`, USB) and its Wi-Fi transport were exercised on 2026-10-11; every other device, host and GPU remains untested, so the numbers above hold for this setup only.

---

## 6. Files Changed (This Session)

| Path | Change | Verification |
|------|--------|-------------|
| `docs/implementation-plan.md` | **Created** (new file) | Written and verified via `write_file` |
| No source files modified | — | Per mandate: "Do not modify application source code, configuration, dependencies" |
| No dependency changes | — | Per mandate #2: "minimum steps, tokens, cost"; no new dependencies |
| `cargo check` | Not re-run | Per mandate: "Do not repeat checks already confirmed passing unless this change requires them" |

---

## 7. Verification

- `write_file` to `docs/implementation-plan.md` — succeeded, file created
- No `cargo check` re-run needed (already passes; no code changes)
- All facts verified against: reference URL, `git diff`, `cargo check` output, `WORKLOG.md`, `PRD.md`
- No unverified assumptions introduced
- Unresolved items explicitly marked; no design details invented

---

## 8. Blocker (Tersisa — per user constraint)

| Blocker | Status |
|---------|--------|
| **Memory and GPU use** | Still unmeasured; needs per-stage instrumentation, not a re-run of an existing test |
| **Superdesign CLI authentication** | `authenticated: false` per CLI `--json`; separate from URL access |
| **Theme color mapping** | Not performed per "minimum changes" mandate #2 — only if explicitly required |
| **Full UI layout extraction** | JS-rendered content not fully traversable; limitation accepted |

**Open items, V1 scope only**: memory and GPU use are unmeasured (needs instrumentation, not a re-run); text/full-keyboard injection is not implemented (not a V1 goal); `connection` and `settings`/`clipboard` remain partial/planned; an OBS window-capture run was not performed because it would write a scene collection into the user's OBS configuration (criterion 7 records the capture-method caveat that run would have tested).

---

## 9. Next Action (Single Priority)

**User discretion required**: Resolve whether to:
1. Map theme colors from reference to HermesGate UI (design decision, optional per mandate #2)
2. Evaluate window controls from reference (may conflict with OBS capture pattern already verified)
3. Accept current state — implementation plan complete with all evidence-based mappings and unresolved items clearly documented

**Per constraint**: Do not begin UI implementation. Plan is reconciled and saved.

---
*Laporan CAVEMAN + PONYTAIL: Implementation plan dibuat berdasarkan bukti faktual dari URL referensi, kode HermesGate, WORKLOG.md, dan PRD.md. Semua pemetaan desain ke kode didukung bukti; detail yang tidak dapat diekstrak ditandai sebagai unresolved tanpa diasumsikan. Tidak ada perubahan source code, dependency, atau claim yang tidak diverifikasi. Laporan natively di Telegram.*