// SPDX-License-Identifier: GPL-3.0-or-later

//! the settings root: destinations, then the instance. playing is the
//! splash's job.

use std::fs;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::{flat_button, open_folder_button, row_prefix};
use crate::config::Paths;
use crate::launch;

/// one destination: icon, name, chevron.
fn nav_button(title: &str, icon: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("pill");
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(14)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    content.append(&row_prefix(icon));
    let label = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .hexpand(true)
        .build();
    content.append(&label);
    let chevron = gtk::Image::from_icon_name("go-next-symbolic");
    chevron.add_css_class("dim-label");
    content.append(&chevron);
    button.set_child(Some(&content));
    button
}

pub fn build(open_page: Rc<dyn Fn(&str)>) -> gtk::Widget {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let heading = gtk::Label::builder()
        .label("Settings")
        .css_classes(["title-2"])
        .halign(gtk::Align::Start)
        .build();
    root.append(&heading);

    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();
    for (kind, title, icon) in [
        ("graphics", "Graphics", "video-display-symbolic"),
        ("content", "Content", "applications-graphics-symbolic"),
        ("fflags", "FFlags", "power-profile-performance-symbolic"),
        ("extras", "Extras", "starred-symbolic"),
        ("runner", "Advanced", "system-run-symbolic"),
    ] {
        let button = nav_button(title, icon);
        let open_page = open_page.clone();
        button.connect_clicked(move |_| open_page(kind));
        list.append(&button);
    }
    root.append(&list);

    root.append(&instance_group());
    root.upcast()
}

/// whether a Roblox instance is up, and the way out of a wedged one.
fn instance_group() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Instance").build();

    let row = adw::ActionRow::builder().title("Roblox").build();
    row.add_prefix(&row_prefix("system-run-symbolic"));
    let kill = flat_button("Force kill");
    kill.add_css_class("destructive-action");
    row.add_suffix(&kill);
    group.add(&row);

    let logs = adw::ActionRow::builder().title("Logs").build();
    logs.add_prefix(&row_prefix("text-x-generic-symbolic"));
    logs.add_suffix(&open_folder_button(Paths::get().logs()));
    group.add(&logs);

    let refresh: Rc<dyn Fn()> = {
        let row = row.clone();
        let kill = kill.clone();
        Rc::new(move || match launch::running_pid() {
            Some(pid) => {
                row.set_subtitle(&format!("Running (process {pid})"));
                kill.set_sensitive(true);
            }
            None => {
                row.set_subtitle("Not running.");
                kill.set_sensitive(false);
            }
        })
    };
    refresh();

    kill.connect_clicked(glib::clone!(
        #[strong]
        refresh,
        move |_| {
            if let Some(pid) = launch::running_pid() {
                launch::force_kill(pid);
                let _ = fs::remove_file(Paths::get().game_pid());
            }
            refresh();
        }
    ));

    // keep the answer live while the page is on screen.
    let watched = row.downgrade();
    glib::timeout_add_seconds_local(
        2,
        glib::clone!(
            #[strong]
            refresh,
            move || {
                match watched.upgrade() {
                    Some(row) if row.root().is_some() => {
                        refresh();
                        glib::ControlFlow::Continue
                    }
                    _ => glib::ControlFlow::Break,
                }
            }
        ),
    );

    group
}
