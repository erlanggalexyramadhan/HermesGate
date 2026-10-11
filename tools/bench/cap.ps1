param([string]$Out)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinCap {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
$p = Get-Process mirror_stats -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { Write-Output "no mirror window"; exit 1 }
$h = $p.MainWindowHandle
# Bring the mirror window to the front: a DXGI swap chain window is only in the
# screen DC when it is not covered, and SetForegroundWindow alone is refused
# when this shell is not the foreground process.
$size = New-Object WinCap+RECT
[void][WinCap]::GetClientRect($h, [ref]$size)
$cw = $size.Right - $size.Left; $ch = $size.Bottom - $size.Top
[void][WinCap]::ShowWindow($h, 9)
# Raise and move the window to the top-left corner, but do NOT resize it: a
# window taller than the space below its saved position would be captured with
# its off-screen part as garbage, and resizing a live swap chain leaves the
# freshly grown part undrawn.
[void][WinCap]::SetWindowPos($h, [IntPtr]::new(-1), 0, 0, 0, 0, 0x0041)
[void][WinCap]::SetForegroundWindow($h)
Start-Sleep -Milliseconds 900
$wr = New-Object WinCap+RECT
[void][WinCap]::GetWindowRect($h, [ref]$wr)
Write-Output ("window rect: {0},{1} {2}x{3}" -f $wr.Left, $wr.Top, ($wr.Right - $wr.Left), ($wr.Bottom - $wr.Top))
$r = New-Object WinCap+RECT
[void][WinCap]::GetClientRect($h, [ref]$r)
$pt = New-Object WinCap+POINT
$pt.X = 0; $pt.Y = 0
[void][WinCap]::ClientToScreen($h, [ref]$pt)
$w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
if ($w -le 0 -or $ht -le 0) { Write-Output "bad client rect $w x $ht"; exit 1 }
$bmp = New-Object System.Drawing.Bitmap $w, $ht
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($pt.X, $pt.Y, 0, 0, $bmp.Size)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Output "captured $w x $ht -> $Out"
