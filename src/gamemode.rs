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

/// a stuck daemon must not hold up the launch, or the exit.
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

#[cfg(test)]
mod tests {
    use super::*;

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
