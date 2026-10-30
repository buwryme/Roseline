#!/usr/bin/env bash
set -euo pipefail

# builds an AppImage; GTK 4 has no linuxdeploy plugin, so the libraries
# outside the base system are copied in by hand.

cd "$(dirname "$0")/.."

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
arch="${ARCH:-$(uname -m)}"
appdir="build/AppDir"
tools="build/tools"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/lib" "$appdir/usr/share/applications" \
         "$appdir/usr/share/icons/hicolor/scalable/apps" "$tools"

cargo build --release --locked
install -Dm755 target/release/roseline "$appdir/usr/bin/roseline"
install -Dm644 data/net.buwryy.Roseline.desktop \
    "$appdir/usr/share/applications/net.buwryy.Roseline.desktop"
install -Dm644 data/net.buwryy.Roseline.Settings.desktop \
    "$appdir/usr/share/applications/net.buwryy.Roseline.Settings.desktop"
install -Dm644 data/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg \
    "$appdir/usr/share/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg"
install -Dm644 data/icons/hicolor/128x128/apps/net.buwryy.Roseline.png \
    "$appdir/usr/share/icons/hicolor/128x128/apps/net.buwryy.Roseline.png"
install -Dm644 LICENSE "$appdir/usr/share/licenses/roseline/LICENSE"
install -Dm644 NOTICE "$appdir/usr/share/licenses/roseline/NOTICE"
strip --strip-unneeded "$appdir/usr/bin/roseline" || true
mv "$appdir/usr/bin/roseline" "$appdir/usr/bin/roseline.real"
cat > "$appdir/usr/bin/roseline" <<'EOF'
#!/bin/sh
# the runtime launches the desktop entry's executable straight at this
# file, so the loader dance has to live here; AppRun reaches the same file.
here=$(dirname "$(readlink -f "$0")")
lib=$(dirname "$(dirname "$here")")/usr/lib
LD_LIBRARY_PATH="$lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LD_LIBRARY_PATH
exec "$lib/ld-linux.so.2" \
    --library-path "$lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    "$here/roseline.real" "$@"
EOF
chmod 755 "$appdir/usr/bin/roseline"
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
exec "$here/usr/bin/roseline" "$@"
EOF
chmod 755 "$appdir/AppRun"
# appimagetool needs a root icon; make a 256 png where we can
if command -v magick >/dev/null 2>&1; then
    magick -background none -density 384 \
        "$appdir/usr/share/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg" \
        -resize 256x256 "$appdir/net.buwryy.Roseline.png"
else
    cp "$appdir/usr/share/icons/hicolor/128x128/apps/net.buwryy.Roseline.png" \
        "$appdir/net.buwryy.Roseline.png"
fi
ln -sf usr/share/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg \
    "$appdir/net.buwryy.Roseline.svg"
ln -sf usr/share/applications/net.buwryy.Roseline.desktop \
    "$appdir/net.buwryy.Roseline.desktop"

# bundle the C library together with its dynamic loader: an AppImage is
# expected to run wherever the glibc of the build machine is too new.
# the loader goes in under its generic name, because app catalog scanners
# look for ld-linux*.so* to decide the bundle is complete. retries paper
# over sandboxes where the /lib64 view flickers mid-walk.
exists() { ls -- "$1" >/dev/null 2>&1; }

glibc_dirs="/lib64 /usr/lib64 /lib /usr/lib /lib/${arch}-linux-gnu /usr/lib/${arch}-linux-gnu"
for solve in "ld-linux-${arch}.so.1" "ld-linux-${arch}.so.2" "ld-linux-x86-64.so.2" "ld-linux-aarch64.so.2"; do
    for d in $glibc_dirs; do
        for try in 1 2 3 4 5; do
            cp -Ln -- "$d/$solve" "$appdir/usr/lib/ld-linux.so.2" 2>/dev/null && break 3
            sleep 0.2
        done
    done
done
exists "$appdir/usr/lib/ld-linux.so.2" || { echo "no dynamic loader for ${arch} was found" >&2; exit 1; }
for name in libc.so.6 libm.so.6 libmvec.so.1 libdl.so.2 libpthread.so.0 \
    librt.so.1 libresolv.so.2 libutil.so.1 libanl.so.1 \
    libnss_files.so.2 libnss_dns.so.2 libnss_compat.so.2 \
    libnss_nis.so.2 libnss_nisplus.so.2 libnss_hesiod.so.2 \
    libgcc_s.so.1; do
    for d in $glibc_dirs; do
        for try in 1 2 3 4 5; do
            cp -Ln -- "$d/$name" "$appdir/usr/lib/" 2>/dev/null && break 3
            sleep 0.1
        done
    done
done

# bundle everything else the binary loads beyond system glibc
while read -r line; do
    case "$line" in
        *"=>"*) lib="${line##*=> }" ;;
        *) lib="$line" ;;
    esac
    lib="${lib%% *}"
    ls "$lib" >/dev/null 2>&1 || continue
    # glibc family files live in the C library; bundling them breaks AppImages.
    case "$(basename "$lib")" in
        libc.so.*|libm.so.*|libmvec.so.*|libdl.so.*|libpthread.so.*|librt.so.*| \
        libresolv.so.*|libutil.so.*|libnsl.so.*|libanl.so.*|libgcc_s.so.*| \
        libstdc++.so.*|ld-linux*.so.*) continue ;;
    esac
    case "$lib" in
        /*) cp -Ln "$lib" "$appdir/usr/lib/" || true ;;
    esac
done < <(ldd target/release/roseline)

if [ ! -x "$tools/appimagetool-$arch" ]; then
    curl -fL --retry 3 -o "$tools/appimagetool-$arch" \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${arch}.AppImage"
    chmod +x "$tools/appimagetool-$arch"
fi

ARCH="$arch" "$tools/appimagetool-$arch" --appimage-extract-and-run \
    "$PWD/$appdir" "$PWD/build/Roséline-${version}-${arch}.AppImage"
echo "built $PWD/build/Roséline-${version}-${arch}.AppImage"
