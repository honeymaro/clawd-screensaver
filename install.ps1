<#
.SYNOPSIS
  Builds Clawd Saver and registers it as the current user's screensaver.

.DESCRIPTION
  Copies the release binary to %LOCALAPPDATA%\clawd-saver\clawd-saver.scr and
  points HKCU:\Control Panel\Desktop at it. No administrator rights are needed
  because everything stays under the current user.

  Also installs a pinned copy of ccusage under %LOCALAPPDATA%\clawd-saver\runtime
  and records the absolute path to node.exe beside it. The screensaver runs that
  copy directly, which is both faster than resolving the package with pnpx on
  every refresh and immune to being launched with a PATH that has no node on it.

  Caveat: the Windows screensaver dropdown only enumerates .scr files that live
  in System32, so this one will not appear in that list. It still runs on idle -
  Windows reads the path from the registry, not from the dropdown. Use -System
  (from an elevated prompt) if you want it listed. The dropdown's Settings button
  is out of reach for the same reason, so to choose how far back the counter
  reaches and what Clawd is doing while it counts: right-click
  the .scr and pick Configure, or run
  Start-Process <path> -Verb config. Neither a double-click nor /c on the command
  line will do it - both start the screensaver instead, because .scr files go
  through the shell association and its open verb is "%1" /S.

.PARAMETER Timeout
  Idle minutes before the screensaver starts. Default 5.

.PARAMETER SkipBuild
  Register whatever binary is already built instead of rebuilding.

.PARAMETER SkipRuntime
  Leave the bundled ccusage alone. The screensaver falls back to pnpx / npx.

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
    [switch]$SkipRuntime,
    [switch]$System,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

$desktopKey = 'HKCU:\Control Panel\Desktop'
$installDir = Join-Path $env:LOCALAPPDATA 'clawd-saver'
$installScr = Join-Path $installDir 'clawd-saver.scr'
$runtimeDir = Join-Path $installDir 'runtime'
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

# The pinned package is read out of the Rust source rather than repeated here.
# The runner chain falls back to `pnpx ccusage@<version>`, and a mismatch would
# quietly mean two different ccusages depending on which runner won.
function Get-PinnedCcusage {
    $usageRs = Join-Path $PSScriptRoot 'saver\src\usage.rs'
    $m = [regex]::Match((Get-Content $usageRs -Raw),
                        'const CCUSAGE: &str = "(?<spec>ccusage@(?<version>[^"]+))"')
    if (-not $m.Success) {
        throw "could not read the pinned ccusage version out of $usageRs"
    }
    [pscustomobject]@{ Spec = $m.Groups['spec'].Value; Version = $m.Groups['version'].Value }
}

# What is actually sitting in the runtime directory right now, or $null.
function Get-InstalledCcusage {
    $manifest = Join-Path $runtimeDir 'node_modules\ccusage\package.json'
    if (-not (Test-Path $manifest)) { return $null }
    (Get-Content $manifest -Raw | ConvertFrom-Json).version
}

# Reading the version out of the source only keeps the two copies in step within
# a run that actually installs. -SkipRuntime, or a runtime install that failed
# earlier, can leave an older copy on disk - and since it is the first runner
# tried, it wins over the pinned fallback on every fetch. So the check runs even
# when the install does not. A warning, not a failure: an old ccusage still
# produces a number.
function Assert-RuntimeVersion {
    param([string]$Wanted)
    $have = Get-InstalledCcusage
    if (-not $have -or $have -eq $Wanted) { return }
    Write-Warning "The bundled ccusage is $have, but $Wanted is pinned in saver\src\usage.rs."
    Write-Warning 'It is the first runner tried, so it wins over the pinned fallback.'
    Write-Warning 'Re-run without -SkipRuntime to replace it.'
}

# Older builds let WebView2 choose its own profile directory, which it derives
# from the running module's path - and Windows does not spell that path the same
# way every time. CLAWD-~1.SCR.WebView2 and clawd-saver.scr.WebView2, the 8.3
# short and long spellings of one binary, were found side by side: two profiles
# of the same cached page, tens of megabytes each. The binary now names the
# directory itself; these are what is left behind.
function Remove-LegacyLeftovers {
    # The cache used to be one file for whichever period wrote last. It is now
    # one per period (last-1d.json and friends), so the old name is never read
    # or written again and only invites the question of which is current.
    $stale = Join-Path $installDir 'last.json'
    if (Test-Path $stale) {
        try { Remove-Item $stale -Force -ErrorAction Stop; Write-Host '  removed the pre-period last.json' }
        catch { Write-Warning "  could not remove $stale - $($_.Exception.Message)" }
    }

    Get-ChildItem $installDir -Directory -Filter '*.WebView2' -ErrorAction SilentlyContinue |
        ForEach-Object {
            # Held in their own variables: inside catch, $_ is the ErrorRecord
            # rather than the pipeline item, so $_.Name there is empty.
            $name = $_.Name
            $path = $_.FullName
            $mb = [math]::Round((Get-ChildItem $path -Recurse -File -ErrorAction SilentlyContinue |
                    Measure-Object -Property Length -Sum).Sum / 1MB, 1)
            try {
                Remove-Item $path -Recurse -Force -ErrorAction Stop
                Write-Host ("  reclaimed {0:N1} MB from the old {1}" -f $mb, $name)
            } catch {
                Write-Warning "  $name is in use and was left alone - close the screensaver and re-run"
            }
        }
}

# Installs the copy of ccusage the screensaver prefers over every PATH-dependent
# runner. Never fatal: without it the saver still works, just slower and only
# when it inherits a PATH with a package runner on it.
function Install-Runtime {
    $pin = Get-PinnedCcusage
    $pkg = $pin.Spec

    $node = (Get-Command node -ErrorAction SilentlyContinue).Source
    if (-not $node) { throw 'node is not on PATH' }

    New-Item -ItemType Directory -Force $runtimeDir | Out-Null
    # A private manifest keeps the package manager from walking up the tree and
    # attaching this install to some unrelated project.
    '{"name":"clawd-saver-runtime","version":"0.0.0","private":true}' |
        Set-Content (Join-Path $runtimeDir 'package.json') -Encoding ascii

    Write-Host "Installing $pkg into $runtimeDir ..."
    Push-Location $runtimeDir
    try {
        if (Get-Command pnpm -ErrorAction SilentlyContinue) {
            pnpm add $pkg --prefer-offline | Out-Null
        } elseif (Get-Command npm -ErrorAction SilentlyContinue) {
            npm install $pkg --silent | Out-Null
        } else {
            throw 'neither pnpm nor npm is on PATH'
        }
        if ($LASTEXITCODE -ne 0) { throw "the package manager exited with $LASTEXITCODE" }
    } finally {
        Pop-Location
    }

    $cli = Join-Path $runtimeDir 'node_modules\ccusage\src\cli.js'
    if (-not (Test-Path $cli)) { throw "install reported success but $cli is missing" }

    # Recorded now, while PATH still has node on it. Written without a BOM
    # because the screensaver reads the file as a path and a BOM is not
    # whitespace, so it would survive trimming and break the lookup.
    [System.IO.File]::WriteAllText(
        (Join-Path $runtimeDir 'node.txt'), $node, (New-Object System.Text.UTF8Encoding($false)))

    # Prove the exact command the screensaver will run actually works, and show
    # what it costs, since the whole point of this step is the wall-clock.
    $day = Get-Date -Format 'yyyyMMdd'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $probe = & $node $cli daily --json --since $day --until $day | ConvertFrom-Json
    $sw.Stop()
    if ($LASTEXITCODE -ne 0) { throw "the bundled ccusage exited with $LASTEXITCODE" }

    $bytes = (Get-ChildItem $runtimeDir -Recurse -File -Force | Measure-Object -Property Length -Sum).Sum
    Write-Host ("  node    {0}" -f $node)
    Write-Host ("  size    {0:N1} MB" -f ($bytes / 1MB))
    Write-Host ("  probe   today = `${0:N2} in {1:N2}s" -f $probe.totals.totalCost, $sw.Elapsed.TotalSeconds)

    # Confirms the package manager installed what was asked for, rather than
    # resolving to something else and reporting success.
    Assert-RuntimeVersion -Wanted $pin.Version
}

if ($Uninstall) {
    Write-Host 'Removing Clawd Saver...'
    Remove-ItemProperty $desktopKey -Name 'SCRNSAVE.EXE' -ErrorAction SilentlyContinue
    Set-ItemProperty $desktopKey -Name 'ScreenSaveActive' -Value '0'
    Apply-Settings -Seconds 0 -Active $false
    # The whole directory belongs to us: the binary, the bundled ccusage, the
    # date-keyed cache and the diagnostic log.
    foreach ($p in @($installDir, $systemScr)) {
        if (Test-Path $p) {
            try { Remove-Item $p -Recurse -Force; Write-Host "  removed $p" }
            catch { Write-Warning "  could not remove $p - $($_.Exception.Message)" }
        }
    }
    Write-Host 'Done. The screensaver is unregistered.'
    return
}

$exe = Join-Path $PSScriptRoot 'saver\target\release\clawd-saver.exe'

if (-not $SkipBuild) {
    Write-Host 'Building release binary (size-optimised, LTO - this takes a few minutes)...'
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

Remove-LegacyLeftovers

if (-not $SkipRuntime) {
    try {
        Install-Runtime
    } catch {
        Write-Warning "Could not install the bundled ccusage - $($_.Exception.Message)"
        Write-Warning 'The screensaver will fall back to pnpx / npx, which is slower and needs'
        Write-Warning 'a package runner on PATH. Re-run this script to try again.'
    }
} else {
    try { Assert-RuntimeVersion -Wanted (Get-PinnedCcusage).Version }
    catch { Write-Warning "Could not check the bundled ccusage version - $($_.Exception.Message)" }
}

if ($System) {
    try {
        Copy-Item $exe $systemScr -Force
        Write-Host "Also copied to $systemScr (will appear in the settings dropdown)"
    } catch {
        Write-Warning "Could not write to System32 - run from an elevated prompt for -System. $($_.Exception.Message)"
    }
}

$seconds = [Math]::Max(60, $Timeout * 60)
Set-ItemProperty $desktopKey -Name 'SCRNSAVE.EXE'      -Value $installScr
Set-ItemProperty $desktopKey -Name 'ScreenSaveActive'  -Value '1'
Set-ItemProperty $desktopKey -Name 'ScreenSaveTimeOut' -Value "$seconds"
Apply-Settings -Seconds $seconds -Active $true

Write-Host ''
Write-Host "Registered as the screensaver, starting after $Timeout minute(s) idle."
# Both lines name a shell verb instead of passing a switch. Windows routes .scr
# files through the shell association rather than running them, and scrfile's
# verbs are `open -> "%1" /S` and `config -> "%1"`, so a switch written on the
# command line is discarded either way. `& "<path>" /s` does still start the
# saver, but only because open supplies /S itself - and `& "<path>" /c` does
# exactly the same thing, never reaching the settings dialog. The dropdown that
# would normally offer a Settings button does not list this screensaver unless
# -System was used, so the dialog has to be named here.
Write-Host 'Try it now:   Start-Process "' -NoNewline; Write-Host $installScr -NoNewline; Write-Host '" -Verb open'
Write-Host '              Move the mouse or press a key to dismiss it.'
Write-Host 'Settings:     Start-Process "' -NoNewline; Write-Host $installScr -NoNewline; Write-Host '" -Verb config'
Write-Host '              (or right-click the .scr and pick Configure)'
Write-Host '              how much: today, this week, this month, last 7 or last 30 days'
Write-Host '              what Clawd is doing: mining, the forge, a server rack, night'
Write-Host '              fishing, the receipt, the parcel line, the uplink, the dojo,'
Write-Host '              Duck Hunt, or a different one at every start'
Write-Host 'Remove with:  .\install.ps1 -Uninstall'
