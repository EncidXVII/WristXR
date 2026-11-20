# Copyright (C) 2026 EncidXVII
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds the Windows release zip and its SHA-256 into dist/release/. Run from the repository root.
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
# Before 'Stop': Windows PowerShell 5.1 turns cargo's progress lines on stderr into terminating errors.
cargo build --release -p wxr-app
if ($LASTEXITCODE -ne 0) { throw 'build failed' }
$ErrorActionPreference = 'Stop'

$out = 'dist\release'
$stage = Join-Path $out "WristXR-$version-windows-x64"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\wxr-app.exe (Join-Path $stage 'WristXR.exe')
Copy-Item README.md, LICENSE, NOTICE, THIRD_PARTY_NOTICES.md, 'packaging\windows\Uninstall WristXR.cmd' $stage

$zip = "$stage.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path "$stage\*" -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $(Split-Path $zip -Leaf)" | Set-Content -Encoding ascii "$zip.sha256"
Write-Output "$zip`n$hash"
