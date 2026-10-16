# Headless version of the engine's first-run setup window: converts the user's own extracted
# Skate 3 files (default.xex + data) into the engine's asset format. Same code path as
# skate3setup.exe, without the tkinter file dialog.
param(
    [Parameter(Mandatory)][string]$GameXex,       # ...\game\default.xex
    [Parameter(Mandatory)][string]$PackageDir     # ...\skate3rust-windows-x64
)
$ErrorActionPreference = 'Stop'
$engine = Join-Path $PSScriptRoot '..\third_party\skate-3-rust-engine' | Resolve-Path
$py = Join-Path $engine 'target\package-venv\Scripts\python.exe'
$base = Join-Path $PackageDir 'data'
$exe = Join-Path $PackageDir 'skate3rust.exe'
Push-Location $engine
try {
    $code = @"
import sys
from pathlib import Path
from tools.asset_pipeline.customiser_setup import install
from tools.asset_pipeline.validation_report import summary
root = install(Path(r'$GameXex'), Path(r'$base'), Path(r'$exe'), lambda t: print(t, flush=True), refresh=False)
w = summary(root)
print('READY', root, 'warnings:', len(w), flush=True)
"@
    & $py -c $code
    if ($LASTEXITCODE -ne 0) { throw "setup failed ($LASTEXITCODE)" }
} finally { Pop-Location }
