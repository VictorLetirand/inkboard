<#
.SYNOPSIS
  Builds the application in release mode and produces a portable archive: dist/inkboard-<version>-windows-x64.zip

.DESCRIPTION
  The archive contains the executable, the `ink` CLI, the `inkboard-mcp` MCP server, the README and the per-user install scripts.
  It is not signed: see docs/DISTRIBUTION.md for code signing.
#>
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

cargo build --release -p inkboard-app -p inkboard-cli -p inkboard-mcp
if ($LASTEXITCODE -ne 0) { throw 'The build failed.' }

$version = (Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$name = "inkboard-$version-windows-x64"
$stage = Join-Path 'dist' $name
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null

Copy-Item 'target/release/inkboard.exe' $stage
Copy-Item 'target/release/ink.exe' $stage
Copy-Item 'target/release/inkboard-mcp.exe' $stage
Copy-Item 'README.md' $stage
Copy-Item 'scripts/install-user.ps1', 'scripts/uninstall-user.ps1' $stage

$zip = Join-Path 'dist' "$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
Set-Content -Path "$zip.sha256" -Value "$hash  $name.zip" -Encoding ascii
Write-Host "Archive: $zip"
Write-Host "SHA-256: $hash"
