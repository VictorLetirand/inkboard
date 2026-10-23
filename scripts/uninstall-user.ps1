<#
.SYNOPSIS
  Undoes install-user.ps1: removes the files, the shortcut and the HKCU registry entries.
  Your documents and the %APPDATA%\Inkboard folder (custom shortcuts, autosave) are kept.
#>
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\Inkboard'
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Inkboard.lnk'
if (Test-Path $lnk) { Remove-Item $lnk -Force }
foreach ($ext in '.drawio', '.excalidraw', '.canvas', '.mmd') {
    Remove-ItemProperty -Path "HKCU:\Software\Classes\$ext\OpenWithProgids" -Name 'Inkboard.Document' -ErrorAction SilentlyContinue
}
Remove-Item 'HKCU:\Software\Classes\Inkboard.Document' -Recurse -Force -ErrorAction SilentlyContinue
if (Test-Path $dir) {
    # the script runs from this folder: deferred removal
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-Command', "Start-Sleep 2; Remove-Item -LiteralPath '$dir' -Recurse -Force"
}
Write-Host 'Uninstalled.'
