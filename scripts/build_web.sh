#!/usr/bin/env bash
# Builds the web version of Inkboard: a static page in crates/inkboard-app/web/dist (index.html, .js, .wasm, icons.bin).
#   scripts/build_web.sh                  # WebGL renderer (light, default)
#   scripts/build_web.sh wgpu             # WebGPU with a WebGL fallback
#   RUSTUP_TOOLCHAIN=1.99.1 scripts/build_web.sh   # Rust toolchain to use (default: the one rustup selects)
# Needs: rustup target add wasm32-unknown-unknown ; cargo install trunk wasm-bindgen-cli --locked
set -euo pipefail
cd "$(dirname "$0")/../crates/inkboard-app/web"
args=(build --release)
if [ "${1:-glow}" = "wgpu" ]; then args+=(--features web-wgpu); fi
echo "trunk ${args[*]}"
trunk "${args[@]}"
ls -l dist
