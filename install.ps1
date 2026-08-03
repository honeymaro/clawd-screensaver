<#
.SYNOPSIS
  Builds Clawd Saver and registers it as the current user's screensaver.

.DESCRIPTION
  Copies the release binary to %LOCALAPPDATA%\clawd-saver\clawd-saver.scr and
  points HKCU:\Control Panel\Desktop at it. No administrator rights are needed
  because everything stays under the current user.

  Caveat: the Windows screensaver dropdown only enumerates .scr files that live
  in System32, so this one will not appear in that list. It still runs on idle —
  Windows reads the path from the registry, not from the dropdown. Use -System
  (from an elevated prompt) if you want it listed.

.PARAMETER Timeout
  Idle minutes before the screensaver starts. Default 5.

.PARAMETER SkipBuild
  Register whatever binary is already built instead of rebuilding.

.PARAMETER System
  Also copy the .scr into System32 so it shows up in the settings dropdown.
  Requires an elevated prompt.

.PARAMETER Uninstall
  Unregister the screensaver and delete the installed files.

.EXAMPLE
  .\install.ps1
.EXAMPLE
  .\install.ps1 -Timeout 10
.EXAMPLE
  .\install.ps1 -Uninstall
#>
[CmdletBinding()]
param(
    [int]$Timeout = 5,
    [switch]$SkipBuild,
    [switch]$System,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

$desktopKey = 'HKCU:\Control Panel\Desktop'
$installDir = Join-Path $env:LOCALAPPDATA 'clawd-saver'
$installScr = Join-Path $installDir 'clawd-saver.scr'
$systemScr  = Join-Path $env:SystemRoot 'System32\clawd-saver.scr'

# Nudges the running session so the change applies without signing out.
Add-Type -Namespace ClawdSaver -Name Spi -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true)]
public static extern bool SystemParametersInfo(uint action, uint param, IntPtr pv, uint winIni);
'@

function Apply-Settings {
    param([int]$Seconds, [bool]$Active)
    $SPI_SETSCREENSAVETIMEOUT = 0x000F
    $SPI_SETSCREENSAVEACTIVE  = 0x0011
    $UPDATE_AND_BROADCAST     = 0x0003
    [ClawdSaver.Spi]::SystemParametersInfo(
        $SPI_SETSCREENSAVEACTIVE, $(if ($Active) { 1 } else { 0 }), [IntPtr]::Zero, $UPDATE_AND_BROADCAST) | Out-Null
    if ($Active) {
        [ClawdSaver.Spi]::SystemParametersInfo(
            $SPI_SETSCREENSAVETIMEOUT, $Seconds, [IntPtr]::Zero, $UPDATE_AND_BROADCAST) | Out-Null
    }
}

if ($Uninstall) {
    Write-Host 'Removing Clawd Saver...'
    Remove-ItemProperty $desktopKey -Name 'SCRNSAVE.EXE' -ErrorAction SilentlyContinue
    Set-ItemProperty $desktopKey -Name 'ScreenSaveActive' -Value '0'
    Apply-Settings -Seconds 0 -Active $false
    foreach ($p in @($installScr, $systemScr)) {
        if (Test-Path $p) {
            try { Remove-Item $p -Force; Write-Host "  removed $p" }
            catch { Write-Warning "  could not remove $p — $($_.Exception.Message)" }
        }
    }
    if ((Test-Path $installDir) -and -not (Get-ChildItem $installDir)) { Remove-Item $installDir -Force }
    Write-Host 'Done. The screensaver is unregistered.'
    return
}

$exe = Join-Path $PSScriptRoot 'saver\target\release\clawd-saver.exe'

if (-not $SkipBuild) {
    Write-Host 'Building release binary (size-optimised, LTO — this takes a few minutes)...'
    Push-Location (Join-Path $PSScriptRoot 'saver')
    try { cargo build --release } finally { Pop-Location }
}

if (-not (Test-Path $exe)) {
    throw "Release binary not found at $exe. Run without -SkipBuild first."
}

New-Item -ItemType Directory -Force $installDir | Out-Null
Copy-Item $exe $installScr -Force

$size = (Get-Item $installScr).Length
Write-Host ("Installed {0}  ({1:N0} bytes, {2:N2} MB)" -f $installScr, $size, ($size / 1MB))

if ($System) {
    try {
        Copy-Item $exe $systemScr -Force
        Write-Host "Also copied to $systemScr (will appear in the settings dropdown)"
    } catch {
        Write-Warning "Could not write to System32 — run from an elevated prompt for -System. $($_.Exception.Message)"
    }
}

$seconds = [Math]::Max(60, $Timeout * 60)
Set-ItemProperty $desktopKey -Name 'SCRNSAVE.EXE'      -Value $installScr
Set-ItemProperty $desktopKey -Name 'ScreenSaveActive'  -Value '1'
Set-ItemProperty $desktopKey -Name 'ScreenSaveTimeOut' -Value "$seconds"
Apply-Settings -Seconds $seconds -Active $true

Write-Host ''
Write-Host "Registered as the screensaver, starting after $Timeout minute(s) idle."
Write-Host 'Try it now:   & "' -NoNewline; Write-Host $installScr -NoNewline; Write-Host '" /s'
Write-Host 'Move the mouse or press a key to dismiss it.'
Write-Host 'Remove with:  .\install.ps1 -Uninstall'
