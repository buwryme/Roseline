// SPDX-License-Identifier: GPL-3.0-or-later

//! Feral's GameMode, asked over D-Bus to tune the system while the game runs.
//!
//! the `gamemoderun` wrapper preloads the host's client library, which cannot
//! be trusted against Rosemary's bundled glibc; registering the pid from here
//! needs nothing inside the guest.

use anyhow::{Context as _, Result, bail};
use gtk::gio;
use gtk::glib;
use gtk::glib::prelude::ToVariant;
use std::path::Path;

/// a stuck daemon must not hold up the launch, the exit, or a settings page.
const TIMEOUT_MS: i32 = 2_000;

struct Endpoint {
    name: &'static str,
    path: &'static str,
    interface: &'static str,
}

/// the daemon itself, or its portal inside Flatpak, which maps the sandbox's
/// pids onto the host's.
fn endpoint(sandboxed: bool) -> Endpoint {
    if sandboxed {
        Endpoint {
            name: "org.freedesktop.portal.Desktop",
            path: "/org/freedesktop/portal/desktop",
            interface: "org.freedesktop.portal.GameMode",
        }
    } else {
        Endpoint {
            name: "com.feralinteractive.GameMode",
            path: "/com/feralinteractive/GameMode",
            interface: "com.feralinteractive.GameMode",
        }
    }
}

/// start GameMode for `pid`.
pub fn register(pid: u32) -> Result<()> {
    call("RegisterGame", pid)
}

/// end GameMode for `pid`; the daemon also drops pids that have exited, so
/// this only makes it prompt.
pub fn unregister(pid: u32) -> Result<()> {
    call("UnregisterGame", pid)
}

fn call(method: &str, pid: u32) -> Result<()> {
    let pid = i32::try_from(pid).context("the game's pid does not fit GameMode's")?;
    let endpoint = endpoint(crate::rpc::sandboxed());
    let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .context("there is no session bus for GameMode")?;
    let reply = connection
        .call_sync(
            Some(endpoint.name),
            endpoint.path,
            endpoint.interface,
            method,
            Some(&glib::Variant::tuple_from_iter([pid.to_variant()])),
            Some(glib::VariantTy::new("(i)").unwrap()),
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .context("GameMode is not running")?;
    let (status,) = reply
        .get::<(i32,)>()
        .context("GameMode answered with something unexpected")?;
    if status < 0 {
        bail!("GameMode refused {method} for pid {pid}");
    }
    Ok(())
}

/// whether GameMode can be reached from here. Every route a distro takes:
/// the daemon or the `gamemoderun` wrapper on the PATH (arch, fedora, debian
/// all install these into `/usr/bin`), the client library wherever the
/// library dir sits (`/usr/lib` on arch, `/usr/lib64` on red hat, the
/// triplet dirs like `/usr/lib/x86_64-linux-gnu` on debian and ubuntu, and
/// the `/lib` and `lib32` and `lib64` variants), a daemon already running
/// that neither found, or inside a Flatpak sandbox, the portal's GameMode
/// interface, since the host's paths and bus are closed to it.
pub fn installed() -> bool {
    crate::util::on_path("gamemoded").is_some()
        || crate::util::on_path("gamemoderun").is_some()
        || library()
        || daemon_on_the_bus()
        || (crate::rpc::sandboxed() && portal_game_mode())
}

/// the directories a distro may keep the client library in: the bare, local,
/// multilib and plain variants, plus one level down, where the triplet dirs
/// live.
const LIBRARY_DIRS: &[&str] = &[
    "/usr/lib64",
    "/usr/lib",
    "/lib64",
    "/lib",
    "/usr/lib32",
    "/lib32",
    "/usr/local/lib64",
    "/usr/local/lib",
];

fn library() -> bool {
    let roots: Vec<&Path> = LIBRARY_DIRS.iter().map(|dir| Path::new(dir)).collect();
    library_in_dirs(&roots, "libgamemode.so.0")
}

fn library_in_dirs(roots: &[&Path], library: &str) -> bool {
    roots.iter().any(|dir| dir.join(library).is_file())
        || roots
            .iter()
            .filter_map(|dir| std::fs::read_dir(dir).ok())
            // the triplet dirs sit one level down.
            .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
            .any(|entry| entry.path().join(library).is_file())
}

/// the daemon already running, which the PATH and the library dirs can miss.
/// the bus knows its name even when nothing has activated it yet, and a
/// reply can reach a sandbox even when a game's data cannot.
fn daemon_on_the_bus() -> bool {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    connection
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&"com.feralinteractive.GameMode".to_variant()),
            Some(glib::VariantTy::new("b").unwrap()),
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .is_ok_and(|reply| reply.get::<bool>().unwrap_or(false))
}

/// inside a sandbox the daemon's name is filtered away, so ask the portal
/// instead: it lists `org.freedesktop.portal.GameMode` among its interfaces
/// only when it was built against the host's client library.
fn portal_game_mode() -> bool {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    connection
        .call_sync(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.DBus.Introspectable",
            "Introspect",
            None,
            Some(glib::VariantTy::new("s").unwrap()),
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .is_ok_and(|reply| {
            reply
                .get::<String>()
                .is_some_and(|xml| xml.contains("org.freedesktop.portal.GameMode"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_hides_in_a_triplet_dir() {
        let root = std::env::temp_dir().join(format!("roseline-gamemode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let triplet = root.join("x86_64-linux-gnu");
        std::fs::create_dir_all(&triplet).unwrap();
        std::fs::write(triplet.join("libgamemode.so.0"), b"").unwrap();
        let duplicates = [root.as_path()];

        assert!(library_in_dirs(&duplicates, "libgamemode.so.0"));
        assert!(!library_in_dirs(&duplicates, "libgamemodeauto.so.0"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_library_sits_at_the_top_of_a_lib_dir() {
        let root = std::env::temp_dir().join(format!("roseline-gamemode-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("libgamemode.so.0"), b"").unwrap();
        let roots = [root.as_path()];

        assert!(library_in_dirs(&roots, "libgamemode.so.0"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_host_talks_to_the_daemon() {
        let endpoint = endpoint(false);
        assert_eq!(endpoint.name, "com.feralinteractive.GameMode");
        assert_eq!(endpoint.interface, "com.feralinteractive.GameMode");
    }

    #[test]
    fn flatpak_goes_through_the_portal() {
        let endpoint = endpoint(true);
        assert_eq!(endpoint.name, "org.freedesktop.portal.Desktop");
        assert_eq!(endpoint.path, "/org/freedesktop/portal/desktop");
        assert_eq!(endpoint.interface, "org.freedesktop.portal.GameMode");
    }
}
