# Start the Skate engine dev build with the WoW bridge enabled.
#   tools\run-engine.ps1            normal (F8 toggles driving)
#   tools\run-engine.ps1 -Drive     start already driving the WoW player
param([switch]$Drive, [string]$Package = '20261016-203254-4967')
$ErrorActionPreference = 'Stop'
$eng = Join-Path $PSScriptRoot '..\third_party\skate-3-rust-engine' | Resolve-Path
$rel = Join-Path $eng 'target\release'
$assets = Join-Path $eng "target\release-packages\$Package\skate3rust-windows-x64\data\installations\4f182f0a0aac497d80a0ddc404e06058\assets" | Resolve-Path
Stop-Process -Name skate3rust -Force -ErrorAction SilentlyContinue
$env:SKATECRAFT_BRIDGE = '1'
if ($Drive) { $env:SKATECRAFT_DRIVE = '1' } else { Remove-Item Env:SKATECRAFT_DRIVE -ErrorAction SilentlyContinue }
Start-Process -FilePath "$rel\skate3rust.exe" -ArgumentList '--assets', "`"$($assets.Path)`"", '--test-world', '--mute' -WorkingDirectory $rel `
    -RedirectStandardOutput (Join-Path $PSScriptRoot '..\engine-run.log') -RedirectStandardError (Join-Path $PSScriptRoot '..\engine-run.err')
'engine started'
