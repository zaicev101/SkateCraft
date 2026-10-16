# Start the 3.3.5a client and log in with the account stored by wow-login-setup.ps1, then enter
# the world with the selected character. Meant to be run by the account's owner (you).
#   tools\wow-login.ps1                  start the client, log in, enter the world
#   tools\wow-login.ps1 -NoEnterWorld    stop at the character screen
#   tools\wow-login.ps1 -Attach          the client is already running at the login screen
#   tools\wow-login.ps1 -DryRun          do everything except typing the credentials
param(
    [string]$Wow = 'C:\WOW_335_ONPREM\WOW_CLIENT_335\3.3.5a',
    [switch]$NoEnterWorld,
    [switch]$Attach,
    [switch]$DryRun,
    [int]$LoginScreenSeconds = 9,
    [int]$CharacterScreenSeconds = 8
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms

function Escape-SendKeys([string]$s) {
    # SendKeys treats + ^ % ~ ( ) { } [ ] specially: wrap each in braces
    -join ($s.ToCharArray() | ForEach-Object { if ('+^%~(){}[]'.Contains([string]$_)) { "{$_}" } else { [string]$_ } })
}

$file = Join-Path $env:LOCALAPPDATA 'skatecraft\wow-login.json'
if (-not (Test-Path $file)) { throw "No stored login. Run tools\wow-login-setup.ps1 first." }
$cfg = Get-Content $file -Raw | ConvertFrom-Json
$secure = $cfg.password | ConvertTo-SecureString
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)

try {
    if (-not $Attach) {
        if (Get-Process Wow -ErrorAction SilentlyContinue) { throw 'Wow.exe is already running; close it or use -Attach.' }
        Start-Process -FilePath (Join-Path $Wow 'Wow.exe') -WorkingDirectory $Wow
    }
    $deadline = (Get-Date).AddSeconds(60)
    do { Start-Sleep -Milliseconds 500; $p = Get-Process Wow -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1 } until ($p -or (Get-Date) -gt $deadline)
    if (-not $p) { throw 'The client window did not appear.' }
    if (-not $Attach) { Start-Sleep -Seconds $LoginScreenSeconds }

    $shell = New-Object -ComObject WScript.Shell
    [void]$shell.AppActivate($p.Id)
    Start-Sleep -Milliseconds 700

    if ($DryRun) { 'dry run: client is up and focused, not typing credentials'; return }

    $plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
    [System.Windows.Forms.SendKeys]::SendWait((Escape-SendKeys $cfg.account))
    [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
    [System.Windows.Forms.SendKeys]::SendWait((Escape-SendKeys $plain))
    $plain = $null
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')

    if (-not $NoEnterWorld) {
        Start-Sleep -Seconds $CharacterScreenSeconds
        [void]$shell.AppActivate($p.Id)
        [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')   # Enter World with the selected character
    }
    'login sent'
} finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
}
