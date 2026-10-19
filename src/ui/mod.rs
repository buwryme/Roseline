// SPDX-License-Identifier: GPL-3.0-or-later

//! the windows and their pages.

pub(crate) mod content_page;
pub(crate) mod extras_page;
pub(crate) mod flags_page;
pub(crate) mod graphics_page;
pub(crate) mod runner_page;
pub(crate) mod settings_page;
pub(crate) mod splash;
pub(crate) mod window;

use std::path::PathBuf;

use adw::prelude::*;

/// dimmed leading icon, so a page reads at a glance.
pub(crate) fn row_prefix(icon: &str) -> gtk::Image {
    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("dim-label");
    image
}

/// flat button opening a folder, created first so the click lands somewhere.
pub(crate) fn open_folder_button(dir: PathBuf) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text("Open folder")
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("flat");
    button.connect_clicked(move |_| open_folder(&dir));
    button
}

pub(crate) fn open_folder(dir: &PathBuf) {
    let _ = std::fs::create_dir_all(dir);
    let launcher = gtk::FileLauncher::new(Some(&gtk::gio::File::for_path(dir)));
    launcher.launch(
        None::<&gtk::Window>,
        gtk::gio::Cancellable::NONE,
        |result| {
            if let Err(error) = result {
                eprintln!("roseline: could not open folder: {error}");
            }
        },
    );
}

/// flat button opening a URL with the desktop's handler.
pub(crate) fn open_uri_button(label: &str, uri: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    let uri = uri.to_string();
    button.connect_clicked(move |_| {
        let launcher = gtk::UriLauncher::new(&uri);
        launcher.launch(
            None::<&gtk::Window>,
            gtk::gio::Cancellable::NONE,
            |result| {
                if let Err(error) = result {
                    eprintln!("roseline: could not open link: {error}");
                }
            },
        );
    });
    button
}

/// flat, small button for row ends.
pub(crate) fn flat_button(label: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button
}

/// flat icon button for row ends.
pub(crate) fn icon_flat_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("flat");
    button
}
