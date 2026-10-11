# Verify the mirror window is a normal, movable desktop window.
#
# Reports, for the `HermesGateMirror` window: the GWL_STYLE bits that make a
# window draggable/resizable, the frame vs client size (a caption and borders
# occupy real pixels), the hit test of the top edge (HTCAPTION is what lets the
# user drag it), and whether SetWindowPos actually moves it.
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File tools\bench\hg-window.ps1
$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public struct RECT { public int Left, Top, Right, Bottom; }
public class Win {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", SetLastError=true)] public static extern int GetWindowLong(IntPtr h, int index);
  [DllImport("user32.dll", SetLastError=true)] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", SetLastError=true)] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll", SetLastError=true)] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll", SetLastError=true)] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
}
'@

# FindWindow looks up the class atom by name; enumerating and comparing the
# class of every top-level window is what actually matched this window.
$hwnd = [IntPtr]::Zero
$find = [Win+EnumProc] {
  param($h, $l)
  $name = New-Object System.Text.StringBuilder 256
  [void][Win]::GetClassName($h, $name, 256)
  if ($name.ToString() -eq 'HermesGateMirror') { $script:hwnd = $h; return $false }
  return $true
}
[void][Win]::EnumWindows($find, [IntPtr]::Zero)
if ($hwnd -eq [IntPtr]::Zero) { Write-Output 'mirror window: NOT FOUND'; exit 2 }

$GWL_STYLE = -16
$GWLP_EXSTYLE = -20
$style = [Win]::GetWindowLong($hwnd, $GWL_STYLE)
$ex = [Win]::GetWindowLong($hwnd, $GWLP_EXSTYLE)
$WS_CAPTION = 0x00C00000
$WS_THICKFRAME = 0x00040000
$WS_SYSMENU = 0x00080000
$WS_MINIMIZEBOX = 0x00020000
$WS_MAXIMIZEBOX = 0x00010000
$WS_POPUP = 0x80000000

$wr = New-Object RECT; [void][Win]::GetWindowRect($hwnd, [ref]$wr)
$cr = New-Object RECT; [void][Win]::GetClientRect($hwnd, [ref]$cr)
$owner = 0; [void][Win]::GetWindowThreadProcessId($hwnd, [ref]$owner)

# WM_NCHITTEST in *screen* coordinates: horizontal centre of the window,
# 20 px below its top edge — the middle of the title bar, past the top resizing
# border. 2 (HTCAPTION) is what makes the caption draggable.
$hx = [int]($wr.Left + ($wr.Right - $wr.Left) / 2)
$hy = [int]($wr.Top + 20)
$lp = ([int64]($hx -band 0xFFFF)) -bor ([int64]($hy -band 0xFFFF) -shl 16)
$hit = [Win]::SendMessage($hwnd, 0x0084, [IntPtr]::Zero, [IntPtr]$lp)
Write-Output ("hit test probe (screen {0},{1})" -f $hx, $hy)

Write-Output ("window: 0x{0:x} (pid {1})" -f [int64]$hwnd, $owner)
Write-Output ("style: 0x{0:x8}  exstyle: 0x{1:x8}" -f $style, $ex)
foreach ($p in @(
    @('WS_CAPTION', $WS_CAPTION), @('WS_THICKFRAME', $WS_THICKFRAME),
    @('WS_SYSMENU', $WS_SYSMENU), @('WS_MINIMIZEBOX', $WS_MINIMIZEBOX),
    @('WS_MAXIMIZEBOX', $WS_MAXIMIZEBOX))) {
  $has = ($style -band $p[1]) -eq $p[1]
  Write-Output ("{0,-16} {1}" -f $p[0], $(if ($has) { 'yes' } else { 'NO' }))
}
Write-Output ("{0,-16} {1}" -f 'WS_POPUP', $(if (($style -band $WS_POPUP) -ne 0) { 'yes (borderless!)' } else { 'no' }))
Write-Output ("frame: {0}x{1}  client: {2}x{3}  frame-thickness: {4}x{5}" -f `
  ($wr.Right - $wr.Left), ($wr.Bottom - $wr.Top), ($cr.Right - $cr.Left), ($cr.Bottom - $cr.Top), `
  (($wr.Right - $wr.Left) - ($cr.Right - $cr.Left)), (($wr.Bottom - $wr.Top) - ($cr.Bottom - $cr.Top)))
Write-Output ("hit test at the title bar: {0} (2 = HTCAPTION, draggable)" -f $hit)

# Move it and confirm: a movable window changes position.
$before = "({0},{1})" -f $wr.Left, $wr.Top
[void][Win]::SetWindowPos($hwnd, [IntPtr]::Zero, 200, 150, 0, 0, 0x0001 -bor 0x0004)
Start-Sleep -Milliseconds 400
$after = New-Object RECT; [void][Win]::GetWindowRect($hwnd, [ref]$after)
Write-Output ("position: {0} -> ({1},{2})" -f $before, $after.Left, $after.Top)
