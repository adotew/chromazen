#!/bin/sh
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_dir"

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "wasm-bindgen is required. Install it with:" >&2
  echo "  cargo install wasm-bindgen-cli --version 0.2.126 --locked" >&2
  exit 1
fi

cargo build --release --target wasm32-unknown-unknown
wasm-bindgen \
  target/wasm32-unknown-unknown/release/Chromazen.wasm \
  --out-dir web/pkg \
  --out-name chromazen \
  --target web \
  --no-typescript

echo "Built web/pkg. Serve it with:"
echo "  python3 -m http.server 8080 --directory web"
