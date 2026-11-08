# Renders docs/img/og.png (social preview, 1280x640) from og.html with headless Edge. Run from the repo root.
$edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
$src = (Resolve-Path packaging\brand\og.html).Path
$out = Join-Path (Resolve-Path docs\img).Path 'og.png'
& $edge --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1 --window-size=1280,640 --virtual-time-budget=5000 "--screenshot=$out" "file:///$($src.Replace('\','/'))" | Out-Null
Write-Output $out
