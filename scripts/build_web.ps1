# Builds the web version of Inkboard: a static page in crates/inkboard-app/web/dist (index.html, .js, .wasm, icons.bin).
#   .\scripts\build_web.ps1                      # WebGL renderer (light, default)
#   .\scripts\build_web.ps1 -Renderer wgpu       # WebGPU with a WebGL fallback
#   .\scripts\build_web.ps1 -Toolchain 1.99.1    # Rust toolchain to use (default: the one rustup selects)
# Needs: rustup target add wasm32-unknown-unknown ; cargo install trunk wasm-bindgen-cli --locked
param(
    [ValidateSet('glow', 'wgpu')] [string]$Renderer = 'glow',
    [string]$Toolchain = ''
)
$ErrorActionPreference = 'Stop'
if ($Toolchain) { $env:RUSTUP_TOOLCHAIN = $Toolchain }
Push-Location (Join-Path $PSScriptRoot '..\crates\inkboard-app\web')
try {
    $trunkArgs = @('build', '--release')
    if ($Renderer -eq 'wgpu') { $trunkArgs += @('--features', 'web-wgpu') }
    Write-Host "trunk $($trunkArgs -join ' ')"
    trunk @trunkArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Get-ChildItem dist | Select-Object Name, Length | Format-Table
}
finally {
    Pop-Location
}
