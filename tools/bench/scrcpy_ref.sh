#!/usr/bin/env bash
# usage: scrcpy_ref.sh <config-name> <runs>
# Real scrcpy v4.1 as the reference: same device, same scrolling workload.
set -u
S="/c/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
N="C:/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
ADB="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/binaries/adb.exe"
SCRCPY="C:/Users/Lextronaut/Downloads/scrcpy-win64-v4.1/scrcpy-win64-v4.1/scrcpy.exe"
name="$1"; runs="$2"
[ -f "$SCRCPY" ] || { echo "scrcpy not found at $SCRCPY"; ls -la "$(dirname "$SCRCPY")" | head; exit 1; }

A() { "$ADB" -s bc058b2c "$@" >/dev/null 2>&1; }
A shell am start -a android.settings.SETTINGS
sleep 3

: > "$S/bench-$name.txt"
for i in $(seq 1 "$runs"); do
  ( for j in $(seq 1 300); do
      if [ $((j % 2)) -eq 0 ]; then A shell input swipe 360 1300 360 300 300; else A shell input swipe 360 300 360 1300 300; fi
    done ) & swiper=$!
  cpu=$(powershell -NoProfile -Command "\$p=Start-Process -FilePath '$SCRCPY' -ArgumentList '--serial','bc058b2c','--no-audio','--print-fps','--window-title','scrcpy-ref' -PassThru -RedirectStandardOutput '$N/scrcpy-$name-$i.out' -RedirectStandardError '$N/scrcpy-$name-$i.err'; Start-Sleep -Seconds 25; \$c='gone'; try { \$c=(Get-Process -Id \$p.Id -ErrorAction Stop).CPU } catch {}; \$p.CloseMainWindow() | Out-Null; if (-not \$p.WaitForExit(15000)) { Stop-Process -Id \$p.Id -Force -ErrorAction SilentlyContinue }; \$c" | tr -d '\r')
  kill $swiper 2>/dev/null; wait $swiper 2>/dev/null
  echo "run=$i cpu_s=$cpu out=$N/scrcpy-$name-$i.out" | tee -a "$S/bench-$name.txt"
  sleep 3
done

for f in "$S"/scrcpy-$name-*.out; do
  [ -e "$f" ] || continue
  echo "--- $(basename "$f") ---"
  grep -o "fps: [0-9.]*" "$f" | tail -5
  python - "$f" <<'PY'
import re, sys, statistics as st
text = open(sys.argv[1], errors="replace").read()
vals = [float(v) for v in re.findall(r"fps:\s*([0-9.]+)", text)]
if vals:
    tail = vals[len(vals)*2//3:] or vals
    print(f"samples={len(vals)} mean_fps={st.mean(vals):.1f} tail_fps={st.mean(tail):.1f} max={max(vals):.1f}")
else:
    print("no fps samples")
PY
done
