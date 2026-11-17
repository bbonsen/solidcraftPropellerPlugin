#!/usr/bin/env bash
# Builds propellers-0.2.0.zip, ready for SolidCraft's Preferences > Plugins > Install plugin…
# Needs: rustup target add wasm32-unknown-unknown; cargo install wasm-tools
set -euo pipefail
cd "$(dirname "$0")"
cargo test --quiet
cargo build --release --target wasm32-unknown-unknown
wasm-tools component new target/wasm32-unknown-unknown/release/propellers.wasm -o package/propellers.wasm
wasm-tools validate package/propellers.wasm
rm -f propellers-0.2.0.zip
python3 - <<'PY'
import zipfile
with zipfile.ZipFile("propellers-0.2.0.zip", "w", zipfile.ZIP_DEFLATED) as z:
    for f in ("plugin.json", "propellers.wasm"):
        z.write(f"package/{f}", f"propellers-0.2.0/{f}")
    z.write("LICENSE", "propellers-0.2.0/LICENSE")
PY
echo "built propellers-0.2.0.zip"
