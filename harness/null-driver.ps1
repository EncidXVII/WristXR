# Runs a command against SteamVR's null HMD driver (no headset needed), then restores the user's settings.
# Usage: harness/null-driver.ps1 <exe> [args...]
param([Parameter(Mandatory)][string]$Command, [Parameter(ValueFromRemainingArguments)][string[]]$Arguments = @())
$ErrorActionPreference = 'Stop'
if (Get-Process vrserver -ErrorAction SilentlyContinue) { throw 'SteamVR is running; close it first (the harness swaps its settings).' }
$paths = Get-Content (Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath') -Raw | ConvertFrom-Json
$settings = Join-Path $paths.config[0] 'steamvr.vrsettings'
# A previous run cut short (crash, power loss) leaves backups behind: put the user's files back first.
foreach ($leftover in Get-ChildItem $paths.config[0] -Filter '*.dvr-backup' -ErrorAction SilentlyContinue) {
    Move-Item $leftover.FullName ($leftover.FullName -replace '\.dvr-backup$', '') -Force
    Write-Output "restored leftover $($leftover.Name)"
}
$backup = "$settings.dvr-backup"
Copy-Item $settings $backup -Force
# App registrations (vrmanifest paths) live here; tests register the dev build, so restore it too.
$appconfig = Join-Path $paths.config[0] 'appconfig.json'
$appBackup = "$appconfig.dvr-backup"
$hadAppconfig = Test-Path $appconfig
if ($hadAppconfig) { Copy-Item $appconfig $appBackup -Force }
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
    # Windowed (GUI-subsystem) apps are not waited for by `&`; start explicitly and wait. Output goes via files.
    $out = [IO.Path]::GetTempFileName(); $err = [IO.Path]::GetTempFileName()
    $start = @{ FilePath = $Command; PassThru = $true; NoNewWindow = $true; RedirectStandardOutput = $out; RedirectStandardError = $err }
    if ($Arguments) { $start.ArgumentList = $Arguments }
    $p = Start-Process @start
    $null = $p.Handle  # cache the handle so ExitCode is available after exit
    # WaitForExit, not -Wait: -Wait would also wait for SteamVR if it was started as our child.
    $p.WaitForExit()
    Get-Content $out, $err
    Remove-Item $out, $err -ErrorAction SilentlyContinue
    $code = $p.ExitCode
} finally {
    # SteamVR rewrites its settings while running: stop it completely before restoring, then verify the restore.
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Get-Process vrmonitor, vrserver, vrcompositor, vrdashboard, vrwebhelper, vrstartup -ErrorAction SilentlyContinue | Stop-Process -Force
        Start-Sleep -Milliseconds 500
    } while ((Get-Process vrserver, vrmonitor, vrcompositor -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline)
    $expected = (Get-FileHash $backup).Hash
    Move-Item $backup $settings -Force
    if ((Get-FileHash $settings).Hash -ne $expected) { Write-Error 'steamvr.vrsettings was NOT restored; check it by hand'; $code = 1 }
    if ($hadAppconfig) { Move-Item $appBackup $appconfig -Force } elseif (Test-Path $appconfig) { Remove-Item $appconfig -Force }
}
exit $code
