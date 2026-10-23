<#
.SYNOPSIS
  Per-user install (no administrator rights): copies to %LOCALAPPDATA%\Programs\Inkboard,
  adds a Start menu shortcut and an "Open with" entry for the exchange formats.

.DESCRIPTION
  Writes only under HKCU. Does not change the default application of any extension: "Inkboard"
  simply appears in "Open with". To undo: uninstall-user.ps1.
#>
$ErrorActionPreference = 'Stop'
$src = $PSScriptRoot
$dir = Join-Path $env:LOCALAPPDATA 'Programs\Inkboard'
$exe = Join-Path $dir 'inkboard.exe'

New-Item -ItemType Directory -Force $dir | Out-Null
foreach ($f in 'inkboard.exe', 'ink.exe', 'inkboard-mcp.exe', 'README.md', 'uninstall-user.ps1') {
    $p = Join-Path $src $f
    if (Test-Path $p) { Copy-Item $p $dir -Force }
}

# Start menu shortcut
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Inkboard.lnk'
$sh = New-Object -ComObject WScript.Shell
$s = $sh.CreateShortcut($lnk)
$s.TargetPath = $exe
$s.WorkingDirectory = $dir
$s.Description = 'Inkboard, an offline whiteboard'
$s.Save()

# program id + "Open with"
$prog = 'HKCU:\Software\Classes\Inkboard.Document'
New-Item -Force "$prog\shell\open\command" | Out-Null
Set-ItemProperty "$prog" -Name '(default)' -Value 'Inkboard document'
Set-ItemProperty "$prog\shell\open\command" -Name '(default)' -Value "`"$exe`" `"%1`""
New-Item -Force "$prog\DefaultIcon" | Out-Null
Set-ItemProperty "$prog\DefaultIcon" -Name '(default)' -Value "`"$exe`",0"
foreach ($ext in '.drawio', '.excalidraw', '.canvas', '.mmd') {
    $k = "HKCU:\Software\Classes\$ext\OpenWithProgids"
    New-Item -Force $k | Out-Null
    New-ItemProperty -Path $k -Name 'Inkboard.Document' -Value ([byte[]]@()) -PropertyType Binary -Force | Out-Null
}
Write-Host "Installed to $dir"
Write-Host 'Add this folder to your PATH to use the "ink" command from a terminal.'
