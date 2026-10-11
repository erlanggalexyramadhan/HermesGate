set -u
S="/c/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
N="C:/Users/Lextronaut/AppData/Local/hermes/cache/scratch"
ADB="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/binaries/adb.exe"
EXE="C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/target/release/examples/mirror_stats.exe"
SER="${SER:-192.168.18.7:39583}"
A() { "$ADB" -s "$SER" "$@" >/dev/null 2>&1; }
"$ADB" connect "$SER" >/dev/null 2>&1
A shell svc power stayon true
A shell am start -a android.settings.SETTINGS
A shell input keyevent 224
sleep 3
echo "keyguard: $("$ADB" -s "$SER" shell dumpsys window 2>/dev/null | grep -o 'isKeyguardShowing=[a-z]*' | tr -d '\r' | head -1)"
echo "focus: $("$ADB" -s "$SER" shell dumpsys window 2>/dev/null | grep -o 'mCurrentFocus=[^ ]*' | tr -d '\r' | head -1)"
powershell -NoProfile -Command "
  \$env:HERMESGATE_VIDEO_STATS = '$N/static-wifi.csv'
  \$p = Start-Process -FilePath '$EXE' -ArgumentList '$SER','12' -PassThru -RedirectStandardError '$N/static-wifi.err'
  Start-Sleep -Seconds 12
  Stop-Process -Id \$p.Id -Force -ErrorAction SilentlyContinue
  exit 0" >/dev/null 2>&1 </dev/null
echo "=== Wi-Fi, screen ON, ZERO input, 12 s ==="
cat "$S/static-wifi.csv" 2>/dev/null | tr -d '\r'
echo "err: $(cat "$S/static-wifi.err" 2>/dev/null | tr -d '\r' | tr '\n' '|')"
A shell svc power stayon false
echo "restored stayon=false"
