# Runs a command against SteamVR's null HMD driver (no headset needed), then restores the user's settings.
# Usage: harness/null-driver.ps1 <exe> [args...]
param([Parameter(Mandatory)][string]$Command, [Parameter(ValueFromRemainingArguments)][string[]]$Arguments = @())
$ErrorActionPreference = 'Stop'
if (Get-Process vrserver -ErrorAction SilentlyContinue) { throw 'SteamVR is running; close it first (the harness swaps its settings).' }
$paths = Get-Content (Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath') -Raw | ConvertFrom-Json
$settings = Join-Path $paths.config[0] 'steamvr.vrsettings'
$backup = "$settings.dvr-backup"
Copy-Item $settings $backup -Force
$code = 1
try {
    $s = Get-Content $settings -Raw | ConvertFrom-Json
    function Set-Key($obj, $key, $value) {
        if ($obj.PSObject.Properties[$key]) { $obj.$key = $value } else { $obj | Add-Member -NotePropertyName $key -NotePropertyValue $value }
    }
    foreach ($section in 'steamvr', 'driver_null') {
        if (-not $s.PSObject.Properties[$section]) { $s | Add-Member -NotePropertyName $section -NotePropertyValue ([pscustomobject]@{}) }
    }
    Set-Key $s.steamvr 'forcedDriver' 'null'
    Set-Key $s.steamvr 'enableHomeApp' $false
    Set-Key $s.driver_null 'enable' $true
    [IO.File]::WriteAllText($settings, ($s | ConvertTo-Json -Depth 32))
    & $Command @Arguments
    $code = $LASTEXITCODE
} finally {
    Get-Process vrmonitor, vrserver, vrcompositor, vrdashboard, vrwebhelper, vrstartup -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 2
    Move-Item $backup $settings -Force
}
exit $code
