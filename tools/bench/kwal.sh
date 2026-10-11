#!/usr/bin/env bash
# usage: kwal.sh <config-name> [ENV=VAL ...]
# Static Settings scene: capture the mirror window, compare with the device
# screenshot scaled to the same size (lanczos = the ideal).
set -u
S="/c/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
N="C:/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
ADB="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/binaries/adb.exe"
EXE="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/target/release/examples/mirror_stats.exe"
FF="C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"
name="$1"; shift
A() { "$ADB" -s bc058b2c "$@" >/dev/null 2>&1; }

A shell am start -a android.settings.SETTINGS
sleep 2
for i in 1 2 3 4 5; do A shell input swipe 360 400 360 1500 200; done   # back to the top of the list
sleep 2
A shell screencap -p /sdcard/kwal.png
"$ADB" -s bc058b2c pull /sdcard/kwal.png "$N/device-$name.png" >/dev/null 2>&1

envs=""
for kv in "$@"; do envs="$envs \$env:${kv%%=*}='${kv#*=}';"; done
powershell -NoProfile -Command "$envs \$p=Start-Process -FilePath '$EXE' -ArgumentList 'bc058b2c','60' -PassThru -RedirectStandardOutput '$N/q-$name.out' -RedirectStandardError '$N/q-$name.err'; Start-Sleep -Seconds 8; & '$N/cap.ps1' -Out '$N/mirror-$name.png'; Stop-Process -Id \$p.Id -Force -ErrorAction SilentlyContinue" | tr -d '\r'
sleep 2
echo "--- device-$name.png vs mirror-$name.png ---"
if [ -e "$S/mirror-$name.png" ]; then
  size=$(python -c "
import struct,sys
d=open(r'$N/mirror-$name.png','rb').read(33)
print(struct.unpack('>II', d[16:24]))
" 2>/dev/null)
  echo "mirror size: $size"
  W=$(echo "$size" | tr -d '(),' | awk '{print $1}'); H=$(echo "$size" | tr -d '(),' | awk '{print $2}')
  "$FF" -y -i "$(cygpath -w "$S/device-$name.png")" -vf "scale=${W}:${H}:flags=lanczos" "$(cygpath -w "$S/device-$name-ideal.png")" 2>&1 | tail -1
  "$FF" -i "$(cygpath -w "$S/mirror-$name.png")" -i "$(cygpath -w "$S/device-$name-ideal.png")" -lavfi psnr -f null - 2>&1 | grep -o "average:[a-z0-9.]*"
else
  echo "no mirror capture"
fi
