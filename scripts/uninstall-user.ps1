<#
.SYNOPSIS
  Annule install-user.ps1 : retire les fichiers, le raccourci et les entrées de registre HKCU.
  Les documents et le dossier %APPDATA%\TableauBlanc (raccourcis personnalisés, enregistrement automatique) sont conservés.
#>
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\TableauBlanc'
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Tableau blanc.lnk'
if (Test-Path $lnk) { Remove-Item $lnk -Force }
foreach ($ext in '.drawio', '.excalidraw', '.canvas', '.mmd') {
    Remove-ItemProperty -Path "HKCU:\Software\Classes\$ext\OpenWithProgids" -Name 'TableauBlanc.Document' -ErrorAction SilentlyContinue
}
Remove-Item 'HKCU:\Software\Classes\TableauBlanc.Document' -Recurse -Force -ErrorAction SilentlyContinue
if (Test-Path $dir) {
    # le script tourne depuis ce dossier : suppression différée
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-Command', "Start-Sleep 2; Remove-Item -LiteralPath '$dir' -Recurse -Force"
}
Write-Host 'Désinstallé.'
