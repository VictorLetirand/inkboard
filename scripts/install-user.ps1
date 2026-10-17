<#
.SYNOPSIS
  Installation par utilisateur (sans droits administrateur) : copie dans %LOCALAPPDATA%\Programs\TableauBlanc,
  raccourci dans le menu Démarrer et entrée « Ouvrir avec » pour les formats d'échange.

.DESCRIPTION
  N'écrit que sous HKCU. Ne change pas l'application par défaut d'une extension : « Tableau blanc »
  apparaît simplement dans « Ouvrir avec ». Annulation : uninstall-user.ps1.
#>
$ErrorActionPreference = 'Stop'
$src = $PSScriptRoot
$dir = Join-Path $env:LOCALAPPDATA 'Programs\TableauBlanc'
$exe = Join-Path $dir 'tableau-blanc.exe'

New-Item -ItemType Directory -Force $dir | Out-Null
foreach ($f in 'tableau-blanc.exe', 'wb.exe', 'README.md', 'uninstall-user.ps1') {
    $p = Join-Path $src $f
    if (Test-Path $p) { Copy-Item $p $dir -Force }
}

# raccourci du menu Démarrer
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Tableau blanc.lnk'
$sh = New-Object -ComObject WScript.Shell
$s = $sh.CreateShortcut($lnk)
$s.TargetPath = $exe
$s.WorkingDirectory = $dir
$s.Description = 'Tableau blanc hors ligne'
$s.Save()

# identifiant de programme + « Ouvrir avec »
$prog = 'HKCU:\Software\Classes\TableauBlanc.Document'
New-Item -Force "$prog\shell\open\command" | Out-Null
Set-ItemProperty "$prog" -Name '(default)' -Value 'Document Tableau blanc'
Set-ItemProperty "$prog\shell\open\command" -Name '(default)' -Value "`"$exe`" `"%1`""
New-Item -Force "$prog\DefaultIcon" | Out-Null
Set-ItemProperty "$prog\DefaultIcon" -Name '(default)' -Value "`"$exe`",0"
foreach ($ext in '.drawio', '.excalidraw', '.canvas', '.mmd') {
    $k = "HKCU:\Software\Classes\$ext\OpenWithProgids"
    New-Item -Force $k | Out-Null
    New-ItemProperty -Path $k -Name 'TableauBlanc.Document' -Value ([byte[]]@()) -PropertyType Binary -Force | Out-Null
}
Write-Host "Installé dans $dir"
Write-Host 'Ajoutez ce dossier à votre PATH pour utiliser la commande « wb » depuis un terminal.'
