#!/usr/bin/env bash
set -euo pipefail

# builds an AppImage; GTK 4 has no linuxdeploy plugin, so the libraries
# outside the base system are copied in by hand.

cd "$(dirname "$0")/.."

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
appdir="build/AppDir"
tools="build/tools"
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
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
export LD_LIBRARY_PATH="$here/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
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

# bundle everything the binary loads beyond glibc
while read -r line; do
    case "$line" in
        *"=>"*) lib="${line##*=> }" ;;
        *) lib="$line" ;;
    esac
    lib="${lib%% *}"
    [ -f "$lib" ] || continue
    # these belong to the host's glibc/loader; bundling them breaks AppImages.
    case "$(basename "$lib")" in
        libc.so.*|libm.so.*|libmvec.so.*|libdl.so.*|libpthread.so.*|librt.so.*| \
        libresolv.so.*|libutil.so.*|libnsl.so.*|libanl.so.*|libgcc_s.so.*| \
        ld-linux*.so.*) continue ;;
    esac
    case "$lib" in
        /*) cp -Ln "$lib" "$appdir/usr/lib/" || true ;;
    esac
done < <(ldd target/release/roseline)

if [ ! -x "$tools/appimagetool" ]; then
    curl -L -o "$tools/appimagetool" \
        https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x "$tools/appimagetool"
fi

ARCH=x86_64 "$tools/appimagetool" --appimage-extract-and-run \
    "$appdir" "build/Roseline-${version}-x86_64.AppImage"
echo "built build/Roseline-${version}-x86_64.AppImage"
