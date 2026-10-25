# Builds one variant of the web client into dist-<name>/ (run from anywhere).
#   .\build.ps1 -Renderer glow                 # WebGL (glow), icon pack fetched on demand
#   .\build.ps1 -Renderer wgpu                 # wgpu (WebGPU + WebGL fallback), icon pack fetched on demand
#   .\build.ps1 -Renderer glow -EmbedIcons     # same, with the 8 MB pack inside the .wasm
# Needs: rustup target add wasm32-unknown-unknown; cargo install trunk wasm-bindgen-cli (trunk 0.22 needs rustc >= 1.96: cargo +1.99.1 install trunk --locked)
param(
    [ValidateSet('glow', 'wgpu')] [string]$Renderer = 'glow',
    [switch]$EmbedIcons,
    [switch]$DevProfile
)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$features = "web-$Renderer"
$name = $Renderer
if ($EmbedIcons) { $features += ',embedded-icons'; $name += '-embedded' }
$trunkArgs = @('build', '--no-default-features', '--features', $features, '--dist', "dist-$name")
if (-not $DevProfile) { $trunkArgs += '--release' }
Write-Host "trunk $($trunkArgs -join ' ')"
trunk @trunkArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Get-ChildItem "dist-$name" | Select-Object Name, Length | Format-Table
