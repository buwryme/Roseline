<p align="center">
  <img src="data/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg" width="128" alt="Roséline">
</p>

<h1 align="center">Roséline</h1>

<p align="center">Roblox bootstrapper that runs the macOS builds of Roblox on Linux through <a href="https://github.com/microlift/rosemary">Rosemary</a>.</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.1.2-b23a48?style=flat-square" alt="version">
  <img src="https://img.shields.io/badge/license-GPL--3.0--or--later-6c7a57?style=flat-square" alt="license">
  <a href="https://discord.gg/HrUrzXAxPV">
    <img src="https://img.shields.io/badge/discord-server-5865f2?style=flat-square&logo=discord&logoColor=white" alt="discord server">
  </a>
  <a href="https://github.com/buwryme/Roseline/actions/workflows/ci.yml">
    <img src="https://github.com/buwryme/Roseline/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI">
  </a>
</p>


<p align="center">
  <table>
    <tr>
      <td valign="top" style="padding-right: 8px;">
        <img src="assets/showcase1.png" width="460" alt="Roséline's settings UI, with the about widget open.">
      </td>
      <td valign="top" align="center">
        <img src="assets/showcase3.png" width="380" alt="Roséline running beautifully in a Hyprland desktop."><br><br>
        <img src="assets/showcase2.png" width="380" alt="Roséline's loading splash, for when Roblox is launching">
      </td>
    </tr>
  </table>
</p>

Roséline is a Linux-first Roblox bootstrapper that runs the macOS clients of Roblox using <a href="https://github.com/microlift/rosemary">Rosemary</a>. Usage is similar to <a href="https://sober.vinegarhq.org/">VinegarHQ's Sober</a>, but Roséline introduces lots more customizability, and is *fully* open source. ... oh and also it's written in Rust!!!

## Install

```sh
curl -fsSL https://buwryy.net/api/roseline/setup-script | bash
```

... or non-interactively:

```sh
curl -fsSL https://buwryy.net/api/roseline/setup-script | bash -s -- --system --version v0.1.2 --yes
curl -fsSL https://buwryy.net/api/roseline/setup-script | bash -s -- --flatpak --latest --yes
```

### Or, build manually:

```sh
git clone https://github.com/buwryme/Roseline
cd Roseline
cargo build --release
```

then install system-wide under `/usr/local`:

```sh
sudo make install
```

...or per-user:

```sh
make install PREFIX=~/.local
```

## Cross-distro recipes

- `roseline.spec` - `rpmbuild -bb` against the `make dist` tarball
- `make-deb.sh`
- `PKGBUILD` - `makepkg` on Arch; or `make-arch.sh`
- `net.buwryy.Roseline.yml` — `flatpak-builder --user --install --install-deps-from=flathub`
- `make-appimage.sh`
- `../flake.nix` — `nix build .#`

**switching from a source install to a package**: run `make uninstall` first!!!

## Features

- Automatic client updates, from Roblox's own endpoint
- Server location notifications
- Graphics API and device choices
- Font overrides by regular and monospace fonts; doesn't override language-specific and emoji fonts
- Content overlays (cursors, skyboxes, sounds, textures...)
- Runner management: choose Rosemary versions, reset the Darwin base, set modification policies for the Darwin base...
- Environment variables for Rosemary itself and Mac programs
- Optional Discord RPC, with Join/Show Game buttons and more
- Optional close when leaving a game

## Security

- Session cookies are stored using `libsecret`. Decrypted into plaintext during sessions only, whereas closing the game encrypts it back up and deletes the cookie.
  Existing cookies are migrated automatically. This feature is enforced, for more security.

## Note:

- Games using video playback will crash the game
- Vulkan graphics might be slower than OpenGL due to the Metal2Vulkan translation layer
- Running Roblox on Rosemary is experimental and not as widely tested
- ARM64 Flatpak builds are omitted due to muvm requiring no sandboxes

## Credits

Rosemary and Butter by MicroliftHQ, which is what shaped Roséline

Bloxstrap, for the old avatar editor background

## License

Licensed GPL-3.0-or-later, with additional terms:
- Always keep the credit
- Never misrepresent a derivative as the original
- Zero warranty

<p align="center">made with ♡ by <a href="https://github.com/buwryme">buwryme</a>. join the <a href="https://discord.gg/HrUrzXAxPV">discord server!</a></p>

<p align="center">for inquiries, contact hello@buwryy.net</p>