#!/usr/bin/env bash
set -euo pipefail

# regenerates cargo-sources.json, the crate sources flatpak-builder needs;
# needs python3 with aiohttp and tomlkit, and network.

cd "$(dirname "$0")/.."

python3 - <<'PY'
import aiohttp, tomlkit  # noqa: F401
PY

tool="$(mktemp -d)"
trap 'rm -rf "$tool"' EXIT
git clone --depth 1 https://github.com/flatpak/flatpak-builder-tools "$tool" >/dev/null
python3 "$tool/cargo/flatpak-cargo-generator.py" Cargo.lock -o build-aux/cargo-sources.json
echo "wrote build-aux/cargo-sources.json"
