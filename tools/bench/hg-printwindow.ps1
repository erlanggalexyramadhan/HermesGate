param([string]$Out = "C:/Users/Lextronaut/AppData/Local/hermes/cache/scratch/pw-mirror.png")
# Capture the mirror window through its own HWND with PrintWindow, the way a
# window-capture source (OBS) reads a window: nothing is raised, nothing is
# moved, and covering windows do not matter. Prints the window style bits so
# "no title bar / no frame" is a measurement, not a claim.
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class MirrorPW {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  public static IntPtr Find(string prefix) {
    IntPtr found = IntPtr.Zero;
    EnumWindows(delegate(IntPtr h, IntPtr l) {
      if (!IsWindowVisible(h)) return true;
      StringBuilder sb = new StringBuilder(512);
      GetWindowTextW(h, sb, 512);
      if (sb.ToString().StartsWith(prefix)) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
"@
$h = [MirrorPW]::Find("HermesGate Mirror")
if ($h -eq [IntPtr]::Zero) { Write-Output "no mirror window"; exit 1 }
$style = [MirrorPW]::GetWindowLongPtr($h, -16).ToInt64()
$exstyle = [MirrorPW]::GetWindowLongPtr($h, -20).ToInt64()
$names = @(
  @{ bit = 0x00C00000; name = "WS_CAPTION" },
  @{ bit = 0x00040000; name = "WS_THICKFRAME" },
  @{ bit = 0x00080000; name = "WS_SYSMENU" },
  @{ bit = 0x00800000; name = "WS_BORDER" },
  @{ bit = 0x80000000; name = "WS_POPUP" }
)
$set = ($names | Where-Object { ($style -band $_.bit) -ne 0 } | ForEach-Object { $_.name }) -join "+"
if (-not $set) { $set = "none" }
$wr = New-Object MirrorPW+RECT
$cr = New-Object MirrorPW+RECT
[void][MirrorPW]::GetWindowRect($h, [ref]$wr)
[void][MirrorPW]::GetClientRect($h, [ref]$cr)
$ww = $wr.Right - $wr.Left; $wh = $wr.Bottom - $wr.Top
$cw = $cr.Right - $cr.Left; $ch = $cr.Bottom - $cr.Top
$bmp = New-Object System.Drawing.Bitmap $cw, $ch
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
# 1 = PW_CLIENTONLY, 2 = PW_RENDERFULLCONTENT (required for DWM-composited
# windows, which is what a GPU swap chain is).
$ok = [MirrorPW]::PrintWindow($h, $hdc, 3)
$g.ReleaseHdc($hdc)
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$sum = 0.0; $dark = 0
for ($y = 0; $y -lt $ch; $y += 4) {
  for ($x = 0; $x -lt $cw; $x += 4) {
    $c = $bmp.GetPixel($x, $y)
    $l = 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
    $sum += $l
    if ($l -lt 1) { $dark++ }
  }
}
$n = [math]::Ceiling($cw / 4) * [math]::Ceiling($ch / 4)
Write-Output ("hwnd={0} printwindow={1} window={2}x{3} client={4}x{5} style={6:x8}({7}) exstyle={8:x8} mean_luma={9:n2} all_black_pixels={10:n1}%" -f $h, $ok, $ww, $wh, $cw, $ch, $style, $set, $exstyle, ($sum / $n), (100.0 * $dark / $n))
Write-Output ("saved: " + $Out)
