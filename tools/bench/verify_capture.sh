#!/usr/bin/env bash
# Live capture with a *single* PowerShell invocation.
#
# Why: `powershell ... | tr -d '\r'` only returns once every writer of the pipe
# is gone. A harness launched with Start-Process inherits that pipe, so a
# two-step script (launch, then probe) blocks until the session is over and the
# probe sees nothing — "no mirror window", "process not running". Everything
# that must happen while the session is live has to be inside one invocation,
# and the child has to be killed before it returns (as kwal.sh does).
#
# usage: capture-live.sh <config> [extra env assignments]
set -u
S="/c/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
N="C:/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
ADB="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/binaries/adb.exe"
EXE="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/target/release/examples/mirror_stats.exe"
FF="C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"
name="$1"
envs="${2-}"
A() { "$ADB" -s bc058b2c "$@" >/dev/null 2>&1; }

A shell svc power stayon true
A shell input keyevent 224
sleep 1
"$ADB" -s bc058b2c exec-out screencap -p > "$S/c-$name-dev1.png"

envs="$envs" EXE="$EXE" ADB="$ADB" N="$N" name="$name" S="$S" \
powershell -NoProfile -Command "
  \$env:HERMESGATE_VIDEO_STATS = \$env:stats
  \$(if (\$env:envs) { \$env:envs -split ',' | ForEach-Object { \$kv = \$_ -split '='; Set-Item -Path ('env:' + \$kv[0]) -Value \$kv[1] } })
  \$log = '$N/c-$name-ps.log'
  \$p = Start-Process -FilePath '$EXE' -ArgumentList 'bc058b2c','35' -PassThru -RedirectStandardOutput '$N/c-$name.out' -RedirectStandardError '$N/c-$name.err'
  Start-Sleep -Seconds 4
  & '$ADB' -s bc058b2c shell input keyevent 26 | Out-Null; Start-Sleep -Seconds 2
  & '$ADB' -s bc058b2c shell input keyevent 26 | Out-Null; Start-Sleep -Seconds 2
  & '$ADB' -s bc058b2c shell input keyevent 224 | Out-Null; Start-Sleep -Seconds 2
  & '$N/cap.ps1' -Out '$N/c-$name-mirror.png' | Out-File -FilePath \$log
  Stop-Process -Id \$p.Id -Force
  exit 0
" > /dev/null 2>&1 </dev/null

"$ADB" -s bc058b2c exec-out screencap -p > "$S/c-$name-dev2.png"

echo "--- harness log ---"; tail -4 "$S/c-$name.out" 2>/dev/null
echo "--- decode banner ---"; grep "H.264 decoding" "$S/c-$name.err" 2>/dev/null
echo "--- capture step ---"; cat "$S/c-$name-ps.log" 2>/dev/null | tr -d '\r'
echo "--- pair validity: dev1 vs dev2 (inf = identical) ---"
"$FF" -i "$(cygpath -w "$S/c-$name-dev1.png")" -i "$(cygpath -w "$S/c-$name-dev2.png")" -lavfi psnr -f null - 2>&1 | grep -oE "average:[a-z0-9.]+" | head -1
if [ -s "$S/c-$name-mirror.png" ]; then
  size=$(python -c "
import struct
d=open(r'$N/c-$name-mirror.png','rb').read(33)
print(struct.unpack('>II', d[16:24]))")
  echo "mirror size $size"
  W=$(echo "$size" | tr -d '(),' | awk '{print $1}'); H=$(echo "$size" | tr -d '(),' | awk '{print $2}')
  "$FF" -y -v error -i "$(cygpath -w "$S/c-$name-dev1.png")" -vf "scale=${W}:${H}:flags=lanczos" "$(cygpath -w "$S/c-$name-ideal.png")" 2>/dev/null
  echo -n "PSNR vs lanczos ideal (dev1): "
  "$FF" -i "$(cygpath -w "$S/c-$name-mirror.png")" -i "$(cygpath -w "$S/c-$name-ideal.png")" -lavfi psnr -f null - 2>&1 | grep -o "average:[a-z0-9.]*"
  "$FF" -v error -y -i "$(cygpath -w "$S/c-$name-mirror.png")" -f rawvideo -pix_fmt rgb24 "$N/c-$name.rgb" 2>/dev/null
  python - "$N" "$name" "$W" "$H" <<'PY'
import sys
base, name, W, H = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
d = open(f"{base}/c-{name}.rgb", 'rb').read()
rows, gr, blk = [], [], []
for y in range(H):
    off = y*W*3; r=g=b=0; n=0
    for x in range(0, W, 2):
        i = off+x*3; r+=d[i]; g+=d[i+1]; b+=d[i+2]; n+=1
    r//=n; g//=n; b//=n; rows.append((r,g,b))
    if g>1.35*r and g>1.35*b and g>60: gr.append(y)
    if r+g+b < 12: blk.append(y)
print(f"green_rows={len(gr)} first={gr[0] if gr else None} last={gr[-1] if gr else None}")
print(f"near_black_rows={len(blk)} first={blk[0] if blk else None} last={blk[-1] if blk else None}")
print("row means: " + " | ".join(f"y={y}:{rows[y]}" for y in (0, 2, H//2, H-6, H-1)))
PY
else
  echo "no mirror capture"
fi
