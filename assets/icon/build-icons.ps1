# Regenerates the app icon from the two SVGs in this folder.
#
#   wisp.svg        the full design (rounded tile with the cassette player), used from 40 px up
#   wisp-small.svg  a bolder, simpler version without the tile, used for 16-32 px
#
# Output:
#   wisp.ico        (next to this script) every size in one file; build.rs puts it into wisp.exe and
#                   the installer uses it
#   wisp-128.png    (next to this script) the window icon (ui/app.slint) and the logo in the README
#   png\wisp-<size>.png   64, 128, 256 and 512 px for Linux icon themes and Android
#   ..\..\installer\wizard-*.png   the pictures in the setup wizard (see installer\WispPlayer.iss)
#   ..\..\android\app\src\main\res\drawable-nodpi\ic_launcher_foreground.png   the Android launcher icon
#
# The SVGs are rasterized with the headless Microsoft Edge that ships with Windows, so nothing
# needs to be installed. Run:  powershell -ExecutionPolicy Bypass -File assets\icon\build-icons.ps1

$ErrorActionPreference = "Stop"
$here = $PSScriptRoot

$edge = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) { throw "Microsoft Edge was not found; it is needed to turn the SVGs into PNGs." }

$work = Join-Path $env:TEMP "wisp-icons"
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null

function Convert-SvgToPng([string]$markup, [string]$out, [int]$width, [int]$height) {
    # The SVG goes into the page itself: a page in another folder may not be allowed to load a file.
    $html = Join-Path $work ("page-$width" + "x$height.html")
    "<!doctype html><meta charset=utf-8><style>html,body{margin:0;background:transparent;overflow:hidden}body>svg{display:block;width:${width}px;height:${height}px}</style>$markup" |
        Set-Content -Path $html -Encoding utf8
    $edgeArgs = @("--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--user-data-dir=$work\profile",
                  "--force-device-scale-factor=1", "--default-background-color=00000000",
                  "--window-size=$width,$height", "--screenshot=$out", ([System.Uri]$html).AbsoluteUri)
    $p = Start-Process -FilePath $edge -ArgumentList $edgeArgs -PassThru -WindowStyle Hidden
    if (-not $p.WaitForExit(60000)) { Stop-Process -Id $p.Id -Force }
    if (-not (Test-Path $out)) { throw "Edge produced no image at $width x $height px." }
}

# each icon size and the drawing it comes from
$plan = @(
    @{ Size = 16;  Svg = "wisp-small.svg" }, @{ Size = 20;  Svg = "wisp-small.svg" },
    @{ Size = 24;  Svg = "wisp-small.svg" }, @{ Size = 32;  Svg = "wisp-small.svg" },
    @{ Size = 40;  Svg = "wisp.svg" },       @{ Size = 48;  Svg = "wisp.svg" },
    @{ Size = 64;  Svg = "wisp.svg" },       @{ Size = 128; Svg = "wisp.svg" },
    @{ Size = 256; Svg = "wisp.svg" }
)
foreach ($item in $plan) {
    $png = Join-Path $work "wisp-$($item.Size).png"
    Convert-SvgToPng (Get-Content -Raw -Encoding utf8 (Join-Path $here $item.Svg)) $png $item.Size $item.Size
    $item.Bytes = [IO.File]::ReadAllBytes($png)
}

# An .ico is a small directory followed by the images; every image here is a PNG.
$ms = New-Object IO.MemoryStream
$bw = New-Object IO.BinaryWriter $ms
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$plan.Count)
$offset = 6 + 16 * $plan.Count
foreach ($item in $plan) {
    $dim = if ($item.Size -ge 256) { 0 } else { $item.Size }   # 0 means 256
    $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$item.Bytes.Length); $bw.Write([uint32]$offset)
    $offset += $item.Bytes.Length
}
foreach ($item in $plan) { $bw.Write($item.Bytes) }
$bw.Flush()
[IO.File]::WriteAllBytes((Join-Path $here "wisp.ico"), $ms.ToArray())
[IO.File]::WriteAllBytes((Join-Path $here "wisp-128.png"), ($plan | Where-Object { $_.Size -eq 128 }).Bytes)
# The same drawing as PNG files for Linux icon themes (the Flatpak installs these) and Android.
$pngDir = Join-Path $here "png"
New-Item -ItemType Directory -Force $pngDir | Out-Null
foreach ($size in 64, 128, 256) { [IO.File]::WriteAllBytes((Join-Path $pngDir "wisp-$size.png"), ($plan | Where-Object { $_.Size -eq $size }).Bytes) }
Convert-SvgToPng (Get-Content -Raw -Encoding utf8 (Join-Path $here "wisp.svg")) (Join-Path $pngDir "wisp-512.png") 512 512
# The pictures of the setup wizard: the icon on the app's dark purple, at 100% and 200% scale.
$installerDir = Join-Path $here "..\..\installer"
$full = Get-Content -Raw -Encoding utf8 (Join-Path $here "wisp.svg")
$inner = [regex]::Match($full, '(?s)<svg[^>]*>(.*)</svg>').Groups[1].Value
$large = @"
<svg xmlns="http://www.w3.org/2000/svg" width="164" height="314" viewBox="0 0 164 314">
  <defs><linearGradient id="wizardbg" x1="0" y1="0" x2="0.5" y2="1"><stop offset="0" stop-color="#3b3358"/><stop offset="1" stop-color="#0e0d16"/></linearGradient></defs>
  <rect width="164" height="314" fill="url(#wizardbg)"/>
  <svg x="22" y="64" width="120" height="120" viewBox="0 0 256 256">$inner</svg>
  <text x="82" y="224" text-anchor="middle" font-family="Segoe UI, Arial, sans-serif" font-size="21" font-weight="700" fill="#ffffff">Wisp Player</text>
  <text x="82" y="246" text-anchor="middle" font-family="Segoe UI, Arial, sans-serif" font-size="11" fill="#c9c2e0">gapless audio player</text>
</svg>
"@
# The Android launcher icon has two layers that the system masks into a circle, a squircle...: the dark
# tile is the background (android\app\src\main\res\drawable\ic_launcher_background.xml) and the player
# alone is the foreground, scaled into the 66 dp safe zone of the 108 dp canvas (432 px here).
$player = $inner -replace '<rect x="8" y="8" width="240" height="240" rx="58" fill="url\(#tile\)"/>', '' -replace '<path d="M8 66a58[^>]*fill="url\(#gloss\)"/>', ''
$foreground = @"
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="432" height="432" viewBox="0 0 432 432">
  <g transform="translate(216 216) scale(1.18) translate(-128 -130)">$player</g>
</svg>
"@
$androidRes = [IO.Path]::GetFullPath((Join-Path $here "..\..\android\app\src\main\res\drawable-nodpi"))
New-Item -ItemType Directory -Force $androidRes | Out-Null
Convert-SvgToPng $foreground (Join-Path $androidRes "ic_launcher_foreground.png") 432 432
Convert-SvgToPng $large (Join-Path $installerDir "wizard-large.png") 164 314
Convert-SvgToPng $large (Join-Path $installerDir "wizard-large-2x.png") 328 628
Convert-SvgToPng $full (Join-Path $installerDir "wizard-small.png") 55 55
Convert-SvgToPng $full (Join-Path $installerDir "wizard-small-2x.png") 110 110
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
"wrote wisp.ico ($($plan.Count) sizes), wisp-128.png and the installer pictures"
