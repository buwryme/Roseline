#!/usr/bin/env bash
set -euo pipefail

# assembles an Arch package without makepkg: staged files, .PKGINFO, tar.zst.
# on a real Arch machine use build-aux/PKGBUILD; this is the cross build.

cd "$(dirname "$0")/.."

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
version="${version//-/.}"
pkgrel=1
arch=x86_64
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

make install DESTDIR="$stage" PREFIX=/usr
strip --strip-unneeded "$stage/usr/bin/roseline" || true

cat > "$stage/.PKGINFO" <<EOF
pkgname = roseline
pkgbase = roseline
pkgver = ${version}-${pkgrel}
pkgdesc = Run Roblox with Rosemary on Linux
url = https://github.com/buwryme/Roseline
builddate = $(date +%s)
packager = buwryme <hello@buwryy.net>
size = $(du -sb "$stage" | cut -f1)
arch = ${arch}
license = GPL-3.0-or-later
depend = gtk4
depend = libadwaita
EOF

mkdir -p build
out="build/roseline-${version}-${pkgrel}-${arch}.pkg.tar.zst"
tar -C "$stage" --owner=0 --group=0 --numeric-owner --zstd -cf "$out" .PKGINFO usr
echo "built $out"
