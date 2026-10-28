# SPDX-License-Identifier: GPL-3.0-or-later
{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  libadwaita,
  libsecret,
  glib,
}:

rustPlatform.buildRustPackage {
  pname = "roseline";
  version = "0.1.1-hotfix1";

  src = ./..;
  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
    # build.rs runs glib-compile-resources
    glib
  ];
  buildInputs = [
    gtk4
    libadwaita
    libsecret
    glib
  ];

  postInstall = ''
    install -Dm644 data/net.buwryy.Roseline.desktop $out/share/applications/net.buwryy.Roseline.desktop
    install -Dm644 data/net.buwryy.Roseline.Settings.desktop $out/share/applications/net.buwryy.Roseline.Settings.desktop
    install -Dm644 data/net.buwryy.Roseline.metainfo.xml $out/share/metainfo/net.buwryy.Roseline.metainfo.xml
    install -Dm644 data/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg \
      $out/share/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg
    install -Dm644 data/icons/hicolor/128x128/apps/net.buwryy.Roseline.png \
      $out/share/icons/hicolor/128x128/apps/net.buwryy.Roseline.png
    install -Dm644 LICENSE $out/share/licenses/roseline/LICENSE
    install -Dm644 NOTICE $out/share/licenses/roseline/NOTICE
  '';

  meta = {
    description = "Run Roblox with Rosemary on Linux";
    homepage = "https://github.com/buwryme/Roseline";
    license = lib.licenses.gpl3Plus;
    mainProgram = "roseline";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
}
