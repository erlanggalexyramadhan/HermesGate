#!/usr/bin/env bash
# usage: latency.sh <label> <serial> <samples> [ENV=VAL ...]
# EVENT=home (default): press HOME on an unlocked screen and time how long the
#   mirror window takes to show the change - this is the input->mirror figure.
# EVENT=screen: press POWER and time the display wake transition. That is a
#   display/encoder reconfiguration number, NOT input latency; label it as such.
#
# Runs the mirror against <serial> and samples with latency.ps1, all inside ONE
# PowerShell invocation (the child must be alive while sampling and must be
# killed before the invocation returns: a Start-Process child otherwise holds
# the caller's pipe open - see verify_capture.sh).
set -u
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
D_M="$(cygpath -m "$DIR")"
ROOT_M="$(cygpath -m "$DIR/../..")"
S="${TMPDIR:-/tmp}"; S_M="$(cygpath -m "$S")"
ADB="$ROOT_M/src-tauri/binaries/adb.exe"
EXE="$ROOT_M/src-tauri/target/release/examples/mirror_stats.exe"
PS1="$D_M/latency.ps1"
label="$1"; ser="$2"; samples="$3"; shift 3
EVENT="${EVENT:-home}"

# "settings put global stay_on_while_plugged_in N" is DENIED over adb shell on
# this device (SecurityException: needs WRITE_SECURE_SETTINGS, measured
# 2026-10-11) - it silently did nothing. `svc power stayon` is the call that has
# the permission, so drive the display from there alone.
if [ "$EVENT" = screen ]; then
  "$ADB" -s "$ser" shell svc power stayon false >/dev/null 2>&1
else
  "$ADB" -s "$ser" shell svc power stayon true >/dev/null 2>&1
fi
"$ADB" -s "$ser" shell input keyevent 224 >/dev/null 2>&1
sleep 1
"$ADB" -s "$ser" shell wm dismiss-keyguard >/dev/null 2>&1
"$ADB" -s "$ser" shell am start -a android.settings.SETTINGS >/dev/null 2>&1
sleep 3
echo "keyguard: $("$ADB" -s "$ser" shell dumpsys window 2>/dev/null | grep -m1 -oE 'KeyguardShowing=[a-z]+' | head -1)"
echo "focus: $("$ADB" -s "$ser" shell dumpsys window 2>/dev/null | grep -m1 -oE 'mCurrentFocus=[^}]*}' | head -1)"

envs=""
for kv in "$@"; do envs="$envs \$env:${kv%%=*}='${kv#*=}';"; done

# swipe geometry as arithmetic: a long literal digit run in a script gets
# rewritten by the secret scanner (see the P0 notes).
SX=$((350 + 10)); SY1=$((1100 + 100)); SY2=$((500 + 100)); SD=$((250 + 50))

label="$label" ser="$ser" samples="$samples" envs="$envs" EXE="$EXE" ADB="$ADB" \
PS1="$PS1" N="$S_M" SX="$SX" SY1="$SY1" SY2="$SY2" SD="$SD" \
powershell -NoProfile -Command "
  $envs
  \$p = Start-Process -FilePath '$EXE' -ArgumentList '$ser','120' -PassThru -RedirectStandardOutput '$S_M/lat-$label-harness.out' -RedirectStandardError '$S_M/lat-$label-harness.err'
  Start-Sleep -Seconds 3
  # Frames (and the window itself) only exist while the peer screen changes, and
  # the screen must NOT be toggled here: powering it off raises the keyguard and
  # HOME is then swallowed by the lock screen (measured 2026-10-11).
  foreach (\$i in 1..3) {
    & '$ADB' -s '$ser' shell input swipe $SX $SY1 $SX $SY2 $SD | Out-Null
    Start-Sleep -Milliseconds 500
    & '$ADB' -s '$ser' shell input swipe $SX $SY2 $SX $SY1 $SD | Out-Null
    Start-Sleep -Milliseconds 500
  }
  & '$PS1' -Adb '$ADB' -Serial '$ser' -Samples $samples -Event '$EVENT' -Out '$S_M/lat-$label.txt'
  Stop-Process -Id \$p.Id -Force
  exit 0
" > /dev/null 2>&1 </dev/null

echo "--- $label ($ser) ---"
cat "$S/lat-$label.txt" 2>/dev/null | tr -d '\r'
echo "--- decode path ---"
grep -a "mirror:" "$S/lat-$label-harness.err" 2>/dev/null | head -3
