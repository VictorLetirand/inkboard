<#
.SYNOPSIS
  Installation par utilisateur (sans droits administrateur) : copie dans %LOCALAPPDATA%\Programs\Inkboard,
  raccourci dans le menu Démarrer et entrée « Ouvrir avec » pour les formats d'échange.

.DESCRIPTION
  N'écrit que sous HKCU. Ne change pas l'application par défaut d'une extension : « Inkboard »
  apparaît simplement dans « Ouvrir avec ». Annulation : uninstall-user.ps1.
#>
$ErrorActionPreference = 'Stop'
$src = $PSScriptRoot
$dir = Join-Path $env:LOCALAPPDATA 'Programs\Inkboard'
$exe = Join-Path $dir 'inkboard.exe'

New-Item -ItemType Directory -Force $dir | Out-Null
foreach ($f in 'inkboard.exe', 'ink.exe', 'README.md', 'uninstall-user.ps1') {
    $p = Join-Path $src $f
    if (Test-Path $p) { Copy-Item $p $dir -Force }
}

# raccourci du menu Démarrer
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Inkboard.lnk'
$sh = New-Object -ComObject WScript.Shell
$s = $sh.CreateShortcut($lnk)
$s.TargetPath = $exe
$s.WorkingDirectory = $dir
$s.Description = 'Inkboard, tableau blanc hors ligne'
$s.Save()

# identifiant de programme + « Ouvrir avec »
$prog = 'HKCU:\Software\Classes\Inkboard.Document'
New-Item -Force "$prog\shell\open\command" | Out-Null
Set-ItemProperty "$prog" -Name '(default)' -Value 'Document Inkboard'
Set-ItemProperty "$prog\shell\open\command" -Name '(default)' -Value "`"$exe`" `"%1`""
New-Item -Force "$prog\DefaultIcon" | Out-Null
Set-ItemProperty "$prog\DefaultIcon" -Name '(default)' -Value "`"$exe`",0"
foreach ($ext in '.drawio', '.excalidraw', '.canvas', '.mmd') {
    $k = "HKCU:\Software\Classes\$ext\OpenWithProgids"
    New-Item -Force $k | Out-Null
    New-ItemProperty -Path $k -Name 'Inkboard.Document' -Value ([byte[]]@()) -PropertyType Binary -Force | Out-Null
}
Write-Host "Installé dans $dir"
Write-Host 'Ajoutez ce dossier à votre PATH pour utiliser la commande « ink » depuis un terminal.'
