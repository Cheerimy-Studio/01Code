param([string]$OutPath = "C:\Users\PC\Desktop\01\apps\client\shots\shot.png")
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public struct RECT2 { public int L; public int T; public int R; public int B; }
public class Win32Shot {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT2 r);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
}
"@
[Win32Shot]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path $OutPath) | Out-Null
$p = Get-Process coding01 -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
$h = $p.MainWindowHandle
if ([Win32Shot]::IsIconic($h)) {
  [Win32Shot]::ShowWindow($h, 9) | Out-Null
  Start-Sleep -Milliseconds 600
}
$r = New-Object RECT2
[Win32Shot]::GetWindowRect($h, [ref]$r) | Out-Null
$wd = $r.R - $r.L
$ht = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap($wd, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$dc = $g.GetHdc()
[Win32Shot]::PrintWindow($h, $dc, 2) | Out-Null
$g.ReleaseHdc($dc)
$g.Dispose()
$bmp.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "saved $OutPath ${wd}x${ht}"
