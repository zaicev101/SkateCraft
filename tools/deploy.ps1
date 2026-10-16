# Build the host plugin and install it into the 3.3.5a client folder.
#   tools\deploy.ps1            core only (hot-reloads in the running client)
#   tools\deploy.ps1 -Proxy     also the dinput8.dll proxy (client must be closed)
param([switch]$Proxy, [string]$Wow = 'C:\WOW_335_ONPREM\WOW_CLIENT_335\3.3.5a')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Push-Location $root
try {
    cargo build --release --target i686-pc-windows-msvc -p skatecraft-host-wow -p skatecraft-host-core
    if ($LASTEXITCODE -ne 0) { throw 'build failed' }
    $out = Join-Path $root 'target\i686-pc-windows-msvc\release'
    if ($Proxy) { Copy-Item "$out\dinput8.dll" "$Wow\dinput8.dll" -Force }
    Copy-Item "$out\skatecraft_core.dll" "$Wow\skatecraft_core.dll" -Force
    'deployed'
} finally { Pop-Location }
