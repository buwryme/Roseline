// SPDX-License-Identifier: GPL-3.0-or-later

//! the app logo, bundled so the window has an icon before install.
//!
//! widget icons come from the desktop theme; only the logo is added here.

use gtk::{gdk, gio, glib};

const RESOURCE_PATH: &str = "/net/buwryy/Roseline/icons";

pub fn init() {
    let data = glib::Bytes::from_static(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/roseline.gresource"
    )));
    match gio::Resource::from_data(&data) {
        Ok(resource) => gio::resources_register(&resource),
        Err(error) => {
            eprintln!("roseline: could not load the bundled app icon: {error}");
            return;
        }
    }
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_resource_path(RESOURCE_PATH);
    }
}
