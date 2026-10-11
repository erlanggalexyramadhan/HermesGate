#!/usr/bin/env bash
# Reproduce the first-wireless-session stall: run the harness over Wi-Fi twice -
# once with a live screen and a swipe workload, once with the device screen dark
# (the state the earlier stalled session is suspected to have been in) - and
# print the same counters the stalled session recorded, so the two can be
# compared side by side.
#
# usage: tools/bench/stall-repro.sh   (override the target with SER=<host:port>)
set -u
S="/c/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
N="C:/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
ADB="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/binaries/adb.exe"
EXE="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/target/release/examples/mirror_stats.exe"
SER="${SER:-192.168.18.7:39583}"   # override: SER=<host:port> tools/bench/stall-repro.sh
SECS=12
A() { "$ADB" -s "$SER" "$@" >/dev/null 2>&1; }

run_case() { # $1 label
  local label="$1"
  ( for j in $(seq 1 120); do
      if [ $((j % 2)) -eq 0 ]; then A shell input swipe 360 1300 360 300 300
      else A shell input swipe 360 300 360 1300 300; fi
    done ) & local swiper=$!
  powershell -NoProfile -Command "
    \$env:HERMESGATE_VIDEO_STATS = '$N/repro-$label.csv'
    \$p = Start-Process -FilePath '$EXE' -ArgumentList '$SER','$SECS' -PassThru -RedirectStandardError '$N/repro-$label.err'
    Start-Sleep -Seconds $SECS
    Stop-Process -Id \$p.Id -Force -ErrorAction SilentlyContinue
    exit 0" >/dev/null 2>&1 </dev/null
  kill $swiper 2>/dev/null; wait $swiper 2>/dev/null
  echo "--- $label ---"
  head -1 "$S/repro-$label.csv" 2>/dev/null | tr -d '\r'
  tail -1 "$S/repro-$label.csv" 2>/dev/null | tr -d '\r'
  echo "    decoder banner: $(grep -h 'H.264 decoding' "$S/repro-$label.err" 2>/dev/null | tail -1)"
}

"$ADB" connect "$SER" >/dev/null 2>&1
"$ADB" -s "$SER" get-state 2>&1 | tr -d '\r'
A shell am start -a android.settings.SETTINGS
A shell svc power stayon true
sleep 2
echo "state before: $(A shell dumpsys power 2>/dev/null; "$ADB" -s "$SER" shell dumpsys window 2>/dev/null | grep -E 'isKeyguardShowing' | tr -d '\r' | head -1)"

run_case wifi-screen-on

which_power_on=$(A shell input keyevent 26; "$ADB" -s "$SER" shell dumpsys power 2>/dev/null | grep -E 'mWakefulness=' | tr -d '\r')
echo "after screen-off key: $which_power_on"
sleep 1
run_case wifi-screen-off

A shell input keyevent 26
A shell input keyevent 224
A shell svc power stayon false
sleep 1
echo "restored: $("$ADB" -s "$SER" shell dumpsys power 2>/dev/null | grep -E 'mWakefulness=|mStayOn=' | tr -d '\r' | tr '\n' ' ')"
