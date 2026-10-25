<#
.SYNOPSIS
  Installs the Windows (Installway) installer silently, launches the installed application, then uninstalls it (CI check, release.yml).
.DESCRIPTION
  Usage: pwsh scripts/check_installer_windows.ps1 -Installer <file> -Png <out.png> [-LogDir installer-check]
         pwsh scripts/check_installer_windows.ps1 -FindOnly     (read-only: prints the install folder found in the registry)
  Steps: install (--silent) -> find the install folder (registry "Uninstall" key; never assumed)
         -> inkboard.exe --screenshot + scripts/check_png.py -> silent uninstall -> the folder and the registry entry must be gone.
  Every launch is bounded to 120 s. Installs on the machine it runs on: CI runners only, never on a developer machine.
#>
param(
    [string]$Installer,
    [string]$Png = 'installer-check/screenshot.png',
    [string]$LogDir = 'installer-check',
    [switch]$FindOnly
)
$ErrorActionPreference = 'Stop'
$timeoutMs = 120000

function Unquote([string]$s) { if ($s) { $s.Trim().Trim('"') } }

# Uninstall entries named "Inkboard" (per-user install: HKCU; machine-wide install to Program Files: HKLM, 64 or 32-bit view).
function Get-Entries {
    $roots = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    foreach ($r in $roots) {
        if (Test-Path $r) {
            Get-ChildItem $r | Get-ItemProperty | Where-Object { $_.DisplayName -eq 'Inkboard' }
        }
    }
}

function Find-InstallDir {
    $dir = $null
    $e = @(Get-Entries) | Select-Object -First 1
    if ($e) {
        $dir = Unquote $e.InstallLocation
        if (-not $dir -and $e.DisplayIcon) { $dir = Split-Path (Unquote ($e.DisplayIcon -replace ',\d+$', '')) }
    }
    if ($dir) { $dir.TrimEnd('\') }
}

# Runs a program, waits at most 120 s (kills it past that) and returns its exit code.
function Invoke-Bounded([string]$exe, [string]$arguments, [string]$what) {
    $p = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru -WindowStyle Hidden
    if (-not $p.WaitForExit($timeoutMs)) {
        try { $p.Kill() } catch { }
        throw "${what}: still running after 120 s (killed)"
    }
    $p.WaitForExit()
    return $p.ExitCode
}

function Wait-Gone([string]$path, [int]$seconds) {
    $end = (Get-Date).AddSeconds($seconds)
    while ((Test-Path $path) -and (Get-Date) -lt $end) { Start-Sleep -Seconds 2 }
    -not (Test-Path $path)
}

if ($FindOnly) {
    Write-Host "Install folder: $(Find-InstallDir)"
    exit 0
}

if (-not $Installer) { Write-Host '::error::-Installer is required'; exit 1 }
$Installer = (Resolve-Path $Installer).Path
$Png = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Png)
New-Item -ItemType Directory -Force -Path $LogDir, (Split-Path $Png) | Out-Null

if (@(Get-Entries).Count -gt 0) { Write-Host '::error::Inkboard is already installed on this machine, refusing to continue'; exit 1 }

$dir = $null
$err = $null
try {
    # --- install ---
    $code = Invoke-Bounded $Installer '--silent' 'install'
    if ($code -ne 0) { throw "install exited with code $code" }

    # --- where did it go? ---
    $dir = Find-InstallDir
    if (-not $dir) { throw 'install folder not found in the registry' }
    Write-Host "Install folder: $dir"
    if (-not (Test-Path $dir)) { throw "install folder does not exist: $dir" }
    foreach ($name in 'inkboard.exe', 'ink.exe', 'inkboard-mcp.exe') {
        if (-not (Test-Path (Join-Path $dir $name))) {
            Get-ChildItem $dir -Recurse | Select-Object -ExpandProperty FullName | Write-Host
            throw "$name missing from $dir"
        }
    }

    # --- launch the installed application ---
    $code = Invoke-Bounded (Join-Path $dir 'inkboard.exe') "--screenshot `"$Png`"" 'inkboard --screenshot'
    if ($code -ne 0) { throw "inkboard --screenshot exited with code $code" }
    python scripts/check_png.py $Png
    if ($LASTEXITCODE -ne 0) { throw 'check_png.py rejected the screenshot' }
}
catch {
    $err = $_.Exception.Message
    Write-Host "::error::$err"
}

# --- uninstall (always attempted, even after a failure above) ---
try {
    # uninstall.exe lives outside the install folder (%LOCALAPPDATA%\<publisher>\Uninstall\<product-id>): only the registry knows it
    $entry = @(Get-Entries) | Select-Object -First 1
    $un = if ($entry) { Unquote $entry.UninstallString } else { $null }
    if (-not $un) { throw 'no uninstaller found' }
    # the uninstaller finishes from a copy of itself in %TEMP% and returns at once: do not wait for the process, wait for the folder
    Start-Process -FilePath $un -ArgumentList '--silent' -WindowStyle Hidden
    if ($dir -and -not (Wait-Gone $dir 120)) { throw "install folder still present 120 s after the uninstall: $dir" }
    if (@(Get-Entries).Count -gt 0) { throw 'the "Inkboard" entry is still in the registry after the uninstall' }
    Write-Host 'Uninstalled: folder and registry entry are gone'
}
catch {
    if (-not $err) { $err = $_.Exception.Message }
    Write-Host "::error::$($_.Exception.Message)"
    if ($dir -and (Test-Path $dir)) { Get-ChildItem $dir -Recurse | Select-Object -ExpandProperty FullName | Write-Host }
}

if ($err) { exit 1 }
Write-Host 'OK'
exit 0
