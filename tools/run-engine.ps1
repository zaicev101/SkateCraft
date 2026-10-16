# Start the Skate engine dev build with the WoW bridge enabled (skate mode itself is switched from
# inside WoW with the SkateCraft addon; the plugin starts this script when the addon asks).
#   tools\run-engine.ps1                    normal
#   tools\run-engine.ps1 -Drive             force driving on (tests without the addon)
#   tools\run-engine.ps1 -Difficulty normal easy | normal | hardcore | motorized | custom
param([switch]$Drive, [string]$Package = "20261016-203254-4967", [string]$Difficulty = "normal")
$ErrorActionPreference = 'Stop'
$eng = Join-Path $PSScriptRoot '..\third_party\skate-3-rust-engine' | Resolve-Path
$rel = Join-Path $eng 'target\release'
$assets = Join-Path $eng "target\release-packages\$Package\skate3rust-windows-x64\data\installations\4f182f0a0aac497d80a0ddc404e06058\assets" | Resolve-Path
Stop-Process -Name skate3rust -Force -ErrorAction SilentlyContinue
$env:SKATECRAFT_BRIDGE = '1'
if ($Drive) { $env:SKATECRAFT_DRIVE = '1' } else { Remove-Item Env:SKATECRAFT_DRIVE -ErrorAction SilentlyContinue }
$args = @('--assets', "`"$($assets.Path)`"", '--test-world', '--mute')
if ($Difficulty) { $args += @('--difficulty', $Difficulty) }
Start-Process -FilePath "$rel\skate3rust.exe" -ArgumentList $args -WorkingDirectory $rel `
    -RedirectStandardOutput (Join-Path $PSScriptRoot '..\engine-run.log') -RedirectStandardError (Join-Path $PSScriptRoot '..\engine-run.err')
'engine started'
