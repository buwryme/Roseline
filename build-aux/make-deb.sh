#!/usr/bin/env bash
set -euo pipefail

# builds a .deb with ar and tar alone, no Debian tooling needed.

cd "$(dirname "$0")/.."

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
case "${ARCH:-$(uname -m)}" in
    x86_64) deb_arch=amd64 ;;
    aarch64) deb_arch=arm64 ;;
    *) echo "unsupported architecture: ${ARCH:-$(uname -m)}" >&2; exit 1 ;;
esac
out="build/roseline_${version}_${deb_arch}.deb"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

mkdir -p "$stage/data" "$stage/control"
make install DESTDIR="$stage/data" PREFIX=/usr
strip --strip-unneeded "$stage/data/usr/bin/roseline" || true

installed_size="$(du -sk "$stage/data" | cut -f1)"
cat > "$stage/control/control" <<EOF
Package: roseline
Version: ${version}
Architecture: ${deb_arch}
Maintainer: buwryme <hello@buwryy.net>
Installed-Size: ${installed_size}
Depends: libgtk-4-1, libadwaita-1-0
Section: games
Priority: optional
Homepage: https://github.com/buwryme/Roseline
Description: Run Roblox with Rosemary on Linux
 Roséline is a small GTK4/Libadwaita app written in Rust that runs the macOS Roblox clients via Rosemary. 
EOF

(cd "$stage/data" && find . -type f -printf '%P\0' | xargs -0 md5sum > "$stage/control/md5sums")
printf '2.0\n' > "$stage/debian-binary"
tar -C "$stage/data" -czf "$stage/data.tar.gz" .
tar -C "$stage/control" -czf "$stage/control.tar.gz" .

mkdir -p build
ar rcs "$out" "$stage/debian-binary" "$stage/control.tar.gz" "$stage/data.tar.gz"
echo "built $out"
