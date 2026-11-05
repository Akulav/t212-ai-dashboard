<#
.SYNOPSIS
  Regenerates docs/screenshots/*.png from the real app running in demo mode.

.DESCRIPTION
  Builds Aura, then for each page launches it with AURA_DEMO=1 (built-in
  sample portfolio, no network/keychain access) and AURA_PAGE=<page>, waits
  for it to render, and captures the window with Win32 PrintWindow.
  A throwaway AURA_DATA_DIR is used so your real data is never touched.

.EXAMPLE
  pwsh scripts/screenshots.ps1
  pwsh scripts/screenshots.ps1 -Release -Pages dashboard,dividends
#>
param(
    [string[]]$Pages = @('dashboard', 'holdings', 'dividends', 'insights', 'ai'),
    [switch]$Release,
    [int]$SettleSeconds = 5,
    # Physical pixels; clamped to the primary screen's work area.
    [int]$Width = 1760,
    [int]$Height = 1080
)

$ErrorActionPreference = 'Stop'
# `-File` passes "a,b" as one string; accept comma lists either way.
$Pages = @($Pages | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$root = Split-Path -Parent $PSScriptRoot
$outDir = Join-Path $root 'docs\screenshots'
New-Item -ItemType Directory -Force $outDir | Out-Null

$profile = if ($Release) { 'release' } else { 'debug' }
Push-Location $root
try {
    if ($Release) { cargo build --release -q } else { cargo build -q }
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
} finally { Pop-Location }
$exe = Join-Path $root "target\$profile\aura-portfolio.exe"

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Win {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);

    /// Largest visible top-level window owned by `pid` (winit also creates tiny helper windows).
    public static IntPtr FindMain(uint pid) {
        IntPtr best = IntPtr.Zero; long bestArea = 0;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            RECT r;
            if (p == pid && IsWindowVisible(h) && GetWindowRect(h, out r)) {
                long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
                if (area > bestArea) { bestArea = area; best = h; }
            }
            return true;
        }, IntPtr.Zero);
        return bestArea > 400 * 300 ? best : IntPtr.Zero;
    }
}
'@
[Win]::SetProcessDPIAware() | Out-Null

$names = @{ dashboard = 'dashboard'; holdings = 'holdings'; dividends = 'dividends'; insights = 'insights'; ai = 'ai-engine'; settings = 'settings' }
$dataDir = Join-Path ([IO.Path]::GetTempPath()) "aura-screenshots-$PID"

foreach ($page in $Pages) {
    $env:AURA_DEMO = '1'
    $env:AURA_PAGE = $page
    $env:AURA_DATA_DIR = $dataDir
    $proc = Start-Process -FilePath $exe -PassThru
    try {
        $deadline = (Get-Date).AddSeconds(20)
        $h = [IntPtr]::Zero
        while ($h -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 250
            $h = [Win]::FindMain([uint32]$proc.Id)
        }
        if ($h -eq [IntPtr]::Zero) { throw "window for '$page' never appeared" }
        # Fit the window inside the work area so the taskbar never overlaps it.
        $wa = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
        $width = [Math]::Min($Width, $wa.Width); $height = [Math]::Min($Height, $wa.Height)
        [Win]::SetWindowPos($h, [IntPtr]::Zero, $wa.X, $wa.Y, $width, $height, 0x0040) | Out-Null
        [Win]::SetForegroundWindow($h) | Out-Null
        Start-Sleep -Seconds $SettleSeconds

        # DWMWA_EXTENDED_FRAME_BOUNDS (9) excludes the invisible resize border.
        $r = New-Object Win+RECT
        [Win]::DwmGetWindowAttribute($h, 9, [ref]$r, 16) | Out-Null
        $w = $r.Right - $r.Left; $hgt = $r.Bottom - $r.Top
        # Render the window itself (PW_RENDERFULLCONTENT = 2). Unlike a screen
        # copy this never captures other windows that happen to be on top,
        # because Windows may refuse to bring a background process to front.
        $wr = New-Object Win+RECT
        [Win]::GetWindowRect($h, [ref]$wr) | Out-Null
        $fw = $wr.Right - $wr.Left; $fh = $wr.Bottom - $wr.Top
        $full = New-Object System.Drawing.Bitmap $fw, $fh
        $g = [System.Drawing.Graphics]::FromImage($full)
        $hdc = $g.GetHdc()
        $ok = [Win]::PrintWindow($h, $hdc, 2)
        $g.ReleaseHdc($hdc); $g.Dispose()
        if (-not $ok) { throw "PrintWindow failed for '$page'" }
        # Crop the invisible resize border (window rect vs. DWM frame bounds).
        $bmp = $full.Clone((New-Object System.Drawing.Rectangle ($r.Left - $wr.Left), ($r.Top - $wr.Top), $w, $hgt), $full.PixelFormat)
        $full.Dispose()
        $file = Join-Path $outDir "$($names[$page]).png"
        $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Write-Host "saved $file ($w x $hgt)"
    } finally {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        Remove-Item Env:AURA_DEMO, Env:AURA_PAGE, Env:AURA_DATA_DIR -ErrorAction SilentlyContinue
    }
}
Remove-Item -Recurse -Force $dataDir -ErrorAction SilentlyContinue
