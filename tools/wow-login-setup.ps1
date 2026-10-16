# One-time setup for wow-login.ps1: stores the lab account name and password encrypted for
# THIS Windows user (DPAPI). Nothing is written in plain text and nothing is sent anywhere.
# Run it yourself in a normal PowerShell window; the password is typed at the prompt.
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'skatecraft'
New-Item -ItemType Directory -Force $dir | Out-Null
$account = Read-Host 'WoW account name'
$password = Read-Host 'WoW password (not shown)' -AsSecureString
[pscustomobject]@{
    account  = $account
    password = ($password | ConvertFrom-SecureString)   # DPAPI: only this Windows user can decrypt
} | ConvertTo-Json | Set-Content -Path (Join-Path $dir 'wow-login.json') -Encoding UTF8
"Saved to $dir\wow-login.json (encrypted). Delete the file to forget it."
