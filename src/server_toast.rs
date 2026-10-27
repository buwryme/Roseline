// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::IpAddr;
use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use gtk::gio;
use gtk::glib;
use gtk::glib::prelude::ToVariant;
use serde::Deserialize;

use crate::settings::Settings;

const NOTIFICATION_TIMEOUT: i32 = 6_000;
const URGENCY_NORMAL: u8 = 1;

#[derive(Deserialize)]
struct IpApi {
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    country_name: Option<String>,
}

#[derive(Deserialize)]
struct IpWhois {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    city: Option<String>,
}

struct Location {
    place: String,
    label: String,
}

pub fn helper(log: PathBuf, game_pid: u32) {
    let mut session = crate::session::Session::new(log);
    loop {
        for event in session.tick() {
            if let crate::session::Event::Joining(joining) = event {
                show(joining.server_ip);
            }
        }
        if !std::path::Path::new(&format!("/proc/{game_pid}/cmdline")).is_file() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

pub fn show(ip: Option<IpAddr>) {
    debug(&format!("show {ip:?}"));
    if !Settings::load().server_location.enabled {
        debug("disabled");
        return;
    }
    let Some(ip) = ip else {
        debug("no server address");
        return;
    };
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let location = lookup(ip)?;
            debug(&format!("location {}", location.label));
            notify(&location)?;
            debug("sent the notification");
            Ok::<(), anyhow::Error>(())
        }));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => debug(&format!("{error:#}")),
            Err(_) => debug("the notification thread panicked"),
        }
    });
}

fn debug(message: &str) {
    if std::env::var_os("ROSELINE_TOAST_DEBUG").is_some() {
        eprintln!("roseline-toast: {message}");
    }
}

fn lookup(ip: IpAddr) -> Result<Location> {
    let (sender, receiver) = std::sync::mpsc::channel();
    for source in [Source::IpWho, Source::IpApi] {
        let sender = sender.clone();
        std::thread::spawn(move || {
            if let Ok(location) = source.lookup(ip) {
                let _ = sender.send(location);
            }
        });
    }
    drop(sender);
    receiver
        .recv_timeout(std::time::Duration::from_secs(4))
        .map_err(|_| anyhow::anyhow!("no geolocation service answered for {ip}"))
}

enum Source {
    IpWho,
    IpApi,
}

impl Source {
    fn lookup(self, ip: IpAddr) -> Result<Location> {
        match self {
            Source::IpWho => {
                let info: IpWhois = crate::net::get_json(&format!("https://ipwho.is/{ip}"), &[])
                    .with_context(|| format!("could not look up {ip} with ipwho.is"))?;
                if !info.success {
                    bail!("ipwho.is did not know {ip}");
                }
                place_label(info.city, info.region, info.country)
                    .context("ipwho.is returned no place name")
            }
            Source::IpApi => {
                let info: IpApi =
                    crate::net::get_json(&format!("https://ipapi.co/{ip}/json/"), &[])
                        .with_context(|| format!("could not look up {ip} with ipapi.co"))?;
                place_label(info.city, info.region, info.country_name)
                    .context("ipapi.co returned no place name")
            }
        }
    }
}

fn place_label(
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
) -> Option<Location> {
    let city = city.and_then(nonempty);
    let region = region.and_then(nonempty);
    let country = country.and_then(nonempty);
    let place = city.or(region)?;
    let label = match country {
        Some(country) => format!("{place}, {country}"),
        None => place.clone(),
    };
    Some(Location { place, label })
}

fn nonempty(text: String) -> Option<String> {
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn messages(location: &Location) -> (String, String) {
    (
        format!("Server location: {}", location.label),
        format!("You've joined a server in {}.", location.place),
    )
}

fn notify(location: &Location) -> Result<()> {
    let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .context("there is no session bus for the notification")?;
    let (title, body) = messages(location);
    let mut hints = glib::VariantDict::new(None);
    hints.insert("urgency", URGENCY_NORMAL);
    hints.insert("desktop-entry", crate::config::APP_ID);
    hints.insert("transient", true);
    let parameters = glib::Variant::tuple_from_iter([
        crate::config::DISPLAY_NAME.to_variant(),
        0u32.to_variant(),
        icon().to_variant(),
        title.to_variant(),
        body.to_variant(),
        Vec::<String>::new().to_variant(),
        hints.end(),
        NOTIFICATION_TIMEOUT.to_variant(),
    ]);
    connection
        .call_sync(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
        )
        .context("the notification service refused the message")?;
    Ok(())
}

fn icon() -> String {
    let name = "net.buwryy.Roseline.png";
    for prefix in ["/usr", "/usr/local", "/app"] {
        let path = format!("{prefix}/share/icons/hicolor/128x128/apps/{name}");
        if std::path::Path::new(&path).is_file() {
            return path;
        }
    }
    let development = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("data/icons/hicolor/128x128/apps")
        .join(name);
    if development.is_file() {
        return development.display().to_string();
    }
    crate::config::APP_ID.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_uses_city_then_region() {
        let location = place_label(
            Some("Paris".into()),
            Some("Ile-de-France".into()),
            Some("France".into()),
        )
        .expect("a place");
        assert_eq!(location.place, "Paris");
        assert_eq!(location.label, "Paris, France");

        let location = place_label(None, Some("Ile-de-France".into()), Some("France".into()))
            .expect("a place");
        assert_eq!(location.place, "Ile-de-France");
        assert_eq!(location.label, "Ile-de-France, France");

        assert!(place_label(None, None, None).is_none());
    }

    #[test]
    fn the_copy_names_the_city_and_country() {
        let location = Location {
            place: "Frankfurt am Main".into(),
            label: "Frankfurt am Main, Germany".into(),
        };
        assert_eq!(
            messages(&location),
            (
                "Server location: Frankfurt am Main, Germany".into(),
                "You've joined a server in Frankfurt am Main.".into()
            )
        );
    }
}
