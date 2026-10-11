#!/usr/bin/env bash
# Diagnose the decode-path negotiation in a chosen peer-screen state.
# usage: diag-decode.sh <label> <awake|locked|off>
#   awake  : power stay on, screen on and content visible
#   locked : stay-on off, screen put to sleep before launch, toggled during
#            (keyguard up) = the state the 06:33-06:41 fallback runs ran in
#   off    : stay-on off, screen stays off, no toggle at all
# One PowerShell invocation does launch + toggles + kill (see verify_capture.sh
# for why a two-step script measures nothing).
set -u
S="/c/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
N="C:/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
ADB="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/binaries/adb.exe"
EXE="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/target/release/examples/mirror_stats.exe"
label="$1"; mode="$2"
A() { "$ADB" -s bc058b2c "$@" >/dev/null 2>&1; }

case "$mode" in
  awake)  A shell svc power stayon true   # `settings put global stay_on_while_plugged_in` is DENIED over adb shell (SecurityException, silent no-op)
          A shell input keyevent 224; sleep 2 ;;
  locked) A shell svc power stayon false
          A shell input keyevent 26; sleep 2 ;;
  off)    A shell svc power stayon false
          A shell input keyevent 26; sleep 2 ;;
esac
pre=$("$ADB" -s bc058b2c shell dumpsys power 2>/dev/null | grep -oE "mWakefulness=[A-Za-z]+" | head -1)

label="$label" EXE="$EXE" ADB="$ADB" N="$N" mode="$mode" \
powershell -NoProfile -Command "
  \$env:HERMESGATE_H264_DEBUG = '1'
  \$p = Start-Process -FilePath '$EXE' -ArgumentList 'bc058b2c','25' -PassThru -RedirectStandardOutput '$N/d-$label.out' -RedirectStandardError '$N/d-$label.err'
  Start-Sleep -Seconds 3
  if ('$mode' -eq 'locked') {
    & '$ADB' -s bc058b2c shell input keyevent 26 | Out-Null; Start-Sleep -Seconds 2
    & '$ADB' -s bc058b2c shell input keyevent 26 | Out-Null
  }
  Start-Sleep -Seconds 14
  Stop-Process -Id \$p.Id -Force
  exit 0
" >/dev/null 2>&1 </dev/null

echo "=== $label (mode=$mode, device $pre before launch) ==="
grep -E "negotiation:|offered output types|H.264 decoding on the|accepted the Direct3D|on the CPU \(|retrying" "$S/d-$label.err" | head -14
echo
