#!/usr/bin/env bash
# Build the browser bundle. Needs only cargo and the wasm32 target:
#   rustup target add wasm32-unknown-unknown
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release -p patchferret-wasm --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/patchferret_wasm.wasm web/patchferret.wasm

printf 'web/patchferret.wasm  %s bytes\n' "$(wc -c < web/patchferret.wasm | tr -d ' ')"
echo "Serve web/ over HTTP (module scripts and WASM will not load from file://):"
echo "  python3 -m http.server 8731 --directory web"
