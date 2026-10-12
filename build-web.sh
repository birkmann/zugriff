#!/usr/bin/env bash
# Optional WebGPU client. Desktop downloads and the marketing site need no WASM build.
set -euo pipefail
cd "$(dirname "$0")/game"
command -v trunk >/dev/null || { echo 'Install Trunk: cargo install trunk --locked' >&2; exit 1; }
trunk build --release --public-url "${PUBLIC_URL:-/}" --dist ../prebuilt
