param([string]$Adb, [string]$Serial, [int]$Samples = 5, [string]$Out, [int]$Threshold = 24, [string]$Event = "home",
      # Product-path mode: when -MirrorTitle is set, the window is found by title
      # (the app's mirror window, not a mirror_stats child) and the press is
      # delivered through the session's own control socket by a CDP clicker
      # instead of `adb shell input keyevent`. The sampling method is unchanged.
      [string]$MirrorTitle = "", [string]$Clicker = "", [string]$Trigger = "", [string]$Stamp = "",
      [string]$Label = "Home", [int]$CdpPort = 9223)
# Measure input->mirror latency: time from sending a device input over adb until
# the mirror window visibly changes.
#
# Method: raise the mirror window to the top-left corner (a DXGI swap chain is
# only in the screen DC when it is not covered - see cap.ps1), sample a small
# block of pixels inside the client area from the screen DC, then fire the input
# and poll that block until it differs from the sampled baseline. The figure
# includes the adb input injection and the device's own UI reaction, which is
# what a user of the mirror actually waits for.
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinLat {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  // The mirror window is a second top-level window of the app, so it is not the
  // process's MainWindowHandle: find it by title prefix.
  public static IntPtr FindByTitle(string prefix) {
    IntPtr found = IntPtr.Zero;
    EnumWindows(delegate(IntPtr h, IntPtr l) {
      if (!IsWindowVisible(h)) return true;
      var sb = new System.Text.StringBuilder(512);
      GetWindowTextW(h, sb, 512);
      if (sb.ToString().StartsWith(prefix)) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
"@

# Frames only flow while the peer screen changes, and the window is created
# with the first frame: force motion while waiting for it instead of giving up.
$GWLP_EXSTYLE = -20
$WS_EX_TOPMOST = 0x8
if ($MirrorTitle -ne "") {
  $h = [IntPtr]::Zero
  for ($try = 1; $try -le 12 -and $h -eq [IntPtr]::Zero; $try++) {
    $h = [WinLat]::FindByTitle($MirrorTitle)
    if ($h -eq [IntPtr]::Zero) {
      & $Adb -s $Serial shell input keyevent 3 | Out-Null
      Start-Sleep -Milliseconds 400
      & $Adb -s $Serial shell am start -a android.settings.SETTINGS | Out-Null
      Start-Sleep -Milliseconds 1200
    }
  }
  if ($h -eq [IntPtr]::Zero) { "no window titled '$MirrorTitle' after retries" | Out-File $Out; exit 1 }
  $wasTopmost = (([WinLat]::GetWindowLongPtr($h, $GWLP_EXSTYLE).ToInt64() -band $WS_EX_TOPMOST) -ne 0)
} else {
  $p = $null
  for ($try = 1; $try -le 12 -and -not $p; $try++) {
    $p = Get-Process mirror_stats -ErrorAction SilentlyContinue |
         Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $p) {
      & $Adb -s $Serial shell input keyevent 3 | Out-Null
      Start-Sleep -Milliseconds 400
      & $Adb -s $Serial shell am start -a android.settings.SETTINGS | Out-Null
      Start-Sleep -Milliseconds 1200
    }
  }
  if (-not $p) { "no mirror window after retries" | Out-File $Out; exit 1 }
  $h = $p.MainWindowHandle
  $wasTopmost = $false
}
[void][WinLat]::ShowWindow($h, 9)
[void][WinLat]::SetWindowPos($h, [IntPtr]::new(-1), 0, 0, 0, 0, 0x0041)
[void][WinLat]::SetForegroundWindow($h)
Start-Sleep -Milliseconds 900

$r = New-Object WinLat+RECT
[void][WinLat]::GetClientRect($h, [ref]$r)
$pt = New-Object WinLat+POINT
$pt.X = 0; $pt.Y = 0
[void][WinLat]::ClientToScreen($h, [ref]$pt)
$w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
if ($w -le 0 -or $ht -le 0) { "bad client rect" | Out-File $Out; exit 1 }

$blk = 8
$x0 = [int]($w * 0.15); $y0 = [int]($ht * 0.25)
$bmp = New-Object System.Drawing.Bitmap $blk, $blk
$g = [System.Drawing.Graphics]::FromImage($bmp)
$size = New-Object System.Drawing.Size $blk, $blk

function Sample-Luma {
  [void]$g.CopyFromScreen($pt.X + $x0, $pt.Y + $y0, 0, 0, $size)
  $sum = 0.0; $n = 0
  foreach ($px in 0, 3, 7) {
    foreach ($py in 0, 3, 7) {
      $c = $bmp.GetPixel($px, $py)
      $sum += ($c.R + $c.G + $c.B); $n++
    }
  }
  return ($sum / $n)
}

$mode = if ($MirrorTitle -ne "") { "product (control socket via CDP click '$Label')" } else { "adb" }
$lines = @("event=$Event mode=$mode client ${w}x${ht}, block ${blk}x${blk} at client ($x0,$y0), threshold $Threshold")
$vals = @()
for ($i = 1; $i -le $Samples; $i++) {
  if ($Event -eq "screen") {
    # Keyguard up: HOME is swallowed by the lock screen, but the display wake
    # transition is a large, unambiguous change that goes through the whole
    # pipeline (draw -> capture -> encode -> transport -> decode -> present).
    & $Adb -s $Serial shell input keyevent 26 | Out-Null
    Start-Sleep -Milliseconds 1500
  } else {
    & $Adb -s $Serial shell am start -a android.settings.SETTINGS | Out-Null
    Start-Sleep -Milliseconds 1800
  }
  $base = Sample-Luma
  if ($MirrorTitle -ne "") {
    # Product path. Both clocks are the host clock in UTC milliseconds, so the
    # clicker's own stamp can be subtracted: the clicker prints the instant just
    # before it delivers the click, and everything the clicker does before that
    # (CDP connect, waiting for the trigger) is outside the interval.
    Remove-Item -LiteralPath $Trigger -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $Stamp -ErrorAction SilentlyContinue
    $t0 = [DateTimeOffset]::Now.ToUnixTimeMilliseconds()
    $cp = Start-Process -FilePath "node" -ArgumentList @($Clicker, "$CdpPort", $Label, $Trigger, $Stamp) `
          -PassThru -NoNewWindow -RedirectStandardOutput "$Stamp.out" -RedirectStandardError "$Stamp.err"
    # The clicker waits for this file, then stamps and clicks; it is written after
    # Start-Process returns, so the wait cannot start before the clicker exists.
    New-Item -ItemType File -Path $Trigger -Force | Out-Null
    $hitHost = -1
    while (([DateTimeOffset]::Now.ToUnixTimeMilliseconds() - $t0) -lt 4000) {
      $v = Sample-Luma
      if ([Math]::Abs($v - $base) -gt $Threshold) { $hitHost = [DateTimeOffset]::Now.ToUnixTimeMilliseconds(); break }
    }
    try { $cp | Wait-Process -Timeout 5 -ErrorAction Stop } catch { Stop-Process -Id $cp.Id -Force -ErrorAction SilentlyContinue }
    $inj = -1
    # [long], not [int]: a Unix millisecond stamp overflows Int32 and the cast
    # silently left -1 behind, which read as "no stamp" on every sample.
    if (Test-Path $Stamp) { $inj = [long]((Get-Content $Stamp -Raw).Trim()) }
    $hit = -1.0
    if ($hitHost -ge 0 -and $inj -gt 0) { $hit = [double]($hitHost - $inj) }
    if ($inj -le 0) { $lines += ("  (sample {0}: the clicker wrote no stamp - see $Stamp.err)" -f $i) }
  } else {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    if ($Event -eq "screen") {
      & $Adb -s $Serial shell input keyevent 26 | Out-Null
    } else {
      & $Adb -s $Serial shell input keyevent 3 | Out-Null
    }
    $hit = -1.0
    while ($sw.ElapsedMilliseconds -lt 4000) {
      $v = Sample-Luma
      if ([Math]::Abs($v - $base) -gt $Threshold) { $hit = $sw.Elapsed.TotalMilliseconds; break }
    }
    $sw.Stop()
  }
  $vals += $hit
  $lines += ("sample {0}: baseline_luma={1:N1} change_at={2:N1} ms" -f $i, $base, $hit)
}
$ok = @($vals | Where-Object { $_ -ge 0 } | Sort-Object)
if ($ok.Count -gt 0) {
  $lines += ("n={0} min={1:N1} median={2:N1} max={3:N1} ms (resolution = sampler poll cost; measured 13.3 ms on this host)" -f `
             $ok.Count, $ok[0], $ok[[int]($ok.Count / 2)], $ok[-1])
}
if ($MirrorTitle -ne "" -and -not $wasTopmost) {
  [void][WinLat]::SetWindowPos($h, [IntPtr]::new(-2), 0, 0, 0, 0, 0x0013)
  $nowTop = (([WinLat]::GetWindowLongPtr($h, $GWLP_EXSTYLE).ToInt64() -band $WS_EX_TOPMOST) -ne 0)
  $lines += ("z-order restored: topmost was={0} now={1}" -f $wasTopmost, $nowTop)
}
$g.Dispose(); $bmp.Dispose()
$lines -join "`r`n" | Out-File -FilePath $Out -Encoding utf8
