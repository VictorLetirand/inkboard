#!/usr/bin/env bash
# Builds the browser guest: cargo (wasm32, release) + wasm-bindgen (--target web) into web/pkg.
# `ring` compiles C code for wasm32 and needs a clang with the WebAssembly backend.
# Set CLANG_BIN_DIR to a directory holding clang + llvm-ar (any LLVM >= 15). On the machine
# of the spike, no LLVM was installed, so the Android NDK clang (which includes wasm32) was used.
set -eu
cd "$(dirname "$0")"
if [ -n "${CLANG_BIN_DIR:-}" ]; then
  export CC_wasm32_unknown_unknown="$CLANG_BIN_DIR/clang"
  export AR_wasm32_unknown_unknown="$CLANG_BIN_DIR/llvm-ar"
fi
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/s1_iroh.wasm
ls -l web/pkg/*.wasm
