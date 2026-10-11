#!/usr/bin/env bash
# usage: bench.sh <config-name> <runs> [ENV=VAL ...]
# Runs the mirror_stats harness against a scrolling Settings list (controlled,
# repeatable workload) and records FPS/CPU per run.
set -u
S="/c/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
N="C:/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
ADB="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/binaries/adb.exe"
EXE="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/target/release/examples/mirror_stats.exe"
name="$1"; runs="$2"; shift 2

# Serial may be USB (bc058b2c) or a wireless endpoint (ip:port).
SER="${SERIAL:-bc058b2c}"
A() { "$ADB" -s "$SER" "$@" >/dev/null 2>&1; }
A shell am start -a android.settings.SETTINGS
sleep 3

: > "$S/bench-$name.txt"
for i in $(seq 1 "$runs"); do
  ( for j in $(seq 1 300); do
      if [ $((j % 2)) -eq 0 ]; then A shell input swipe 360 1300 360 300 300; else A shell input swipe 360 300 360 1300 300; fi
    done ) & swiper=$!
  out="$N/stats-$name-$i.csv"; err="$N/err-$name-$i.txt"
  envs=""
  for kv in "$@"; do envs="$envs \$env:${kv%%=*}='${kv#*=}';"; done
  cpu=$(powershell -NoProfile -Command "$envs \$env:HERMESGATE_VIDEO_STATS='$out'; \$p=Start-Process -FilePath '$EXE' -ArgumentList '$SER','60' -PassThru -RedirectStandardOutput '$N/out-$name-$i.txt' -RedirectStandardError '$err'; Start-Sleep -Seconds 25; \$c='gone'; try { \$c=(Get-Process -Id \$p.Id -ErrorAction Stop).CPU } catch {}; Stop-Process -Id \$p.Id -Force -ErrorAction SilentlyContinue; \$c" | tr -d '\r')
  kill $swiper 2>/dev/null; wait $swiper 2>/dev/null
  echo "run=$i cpu_s=$cpu stats=$out" | tee -a "$S/bench-$name.txt"
  sleep 3
done

python_bin="python"
"$python_bin" - "$S" "$name" <<'PY'
import csv, glob, sys, statistics as st
base, name = sys.argv[1], sys.argv[2]
tot = []
for path in sorted(glob.glob(f"{base}/stats-{name}-*.csv")):
    rows = [r for r in csv.DictReader(open(path)) if r.get("t_s")]
    tag = path.split("/")[-1]
    if not rows:
        print(f"{tag}: EMPTY (no frames)"); continue
    last = rows[-1]
    t = float(last["t_s"]); fd = int(last["frames_delivered"]); dec = int(last["frames_decoded"])
    tail = rows[len(rows)*2//3:] or rows
    mean = lambda rs, k: st.mean(float(r[k]) for r in rs if r[k]) if any(r[k] for r in rs) else 0.0
    fps_all = fd / t if t else 0.0
    fps_tail = (int(tail[-1]["frames_delivered"]) - int(tail[0]["frames_delivered"])) / max(1e-6, float(tail[-1]["t_s"]) - float(tail[0]["t_s"]))
    print(f"{tag}: dur={t:.1f}s decoded={dec} delivered={fd} dropped={last['frames_overwritten']} "
          f"avg_fps={fps_all:.1f} tail_fps={fps_tail:.1f} "
          f"read%={mean(rows,'read_pct'):.0f} decode%={mean(rows,'decode_pct'):.0f} "
          f"decode_ms={mean(rows,'decode_ms'):.1f} blit_ms={mean(rows,'blit_ms'):.1f} paint_ms={mean(rows,'paint_ms'):.1f}")
    tot.append(fps_tail)
if tot:
    print(f"MEAN tail_fps over {len(tot)} runs = {st.mean(tot):.1f} (min {min(tot):.1f}, max {max(tot):.1f})")
PY
