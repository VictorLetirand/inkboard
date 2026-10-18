<#
.SYNOPSIS
  Generates the winget manifests of a published release from the templates in packaging/winget.

.DESCRIPTION
  Downloads the Windows installer attached to the GitHub Release of the given tag, computes its SHA-256 and writes the three manifest
  files to dist/winget/manifests/i/Inkboard/Inkboard/<version>. Check them with `winget validate --manifest <folder>` and test the
  install with `winget install --manifest <folder>` (needs `winget settings --enable LocalManifestFiles`), then submit them to
  microsoft/winget-pkgs (for example with `wingetcreate submit`). See docs/DISTRIBUTION.md.

.EXAMPLE
  powershell -File scripts/winget-manifest.ps1 -Tag v0.1.0
#>
param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [string]$Repo = 'VictorLetirand/inkboard'
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

$version = $Tag.TrimStart('v')
$tmp = Join-Path ([IO.Path]::GetTempPath()) "inkboard-winget-$version"
New-Item -ItemType Directory -Force $tmp | Out-Null
$pattern = 'inkboard_*_x64-setup.exe'
if (Get-Command gh -ErrorAction SilentlyContinue) {
    gh release download $Tag --repo $Repo --pattern $pattern --dir $tmp --clobber
    if ($LASTEXITCODE -ne 0) { throw "Could not download the Windows installer from release $Tag." }
} else {
    throw 'The GitHub CLI (gh) is required to find the installer of the release.'
}
$path = (Get-ChildItem $tmp -Filter $pattern | Select-Object -First 1).FullName
$file = Split-Path $path -Leaf
$sha = (Get-FileHash $path -Algorithm SHA256).Hash.ToUpper()
$url = "https://github.com/$Repo/releases/download/$Tag/$file"
$date = (Get-Date).ToString('yyyy-MM-dd')

$out = Join-Path 'dist' "winget/manifests/i/Inkboard/Inkboard/$version"
New-Item -ItemType Directory -Force $out | Out-Null
foreach ($t in Get-ChildItem 'packaging/winget' -Filter '*.yaml') {
    $text = (Get-Content $t.FullName -Raw).Replace('{{VERSION}}', $version).Replace('{{TAG}}', $Tag).Replace('{{URL}}', $url).Replace('{{SHA256}}', $sha).Replace('{{DATE}}', $date)
    Set-Content -Path (Join-Path $out $t.Name) -Value $text -Encoding utf8 -NoNewline
}
Write-Host "Manifests written to $out"
Write-Host "SHA-256: $sha"
