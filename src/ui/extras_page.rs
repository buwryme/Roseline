// SPDX-License-Identifier: GPL-3.0-or-later

//! Discord Rich Presence.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::row_prefix;
use crate::settings::Settings;

pub fn build() -> adw::PreferencesPage {
    let settings = Rc::new(RefCell::new(Settings::load()));
    let page = adw::PreferencesPage::new();

    let group = adw::PreferencesGroup::builder()
        .title("Discord Rich Presence")
        .description("Shows the game on your Discord profile, cover and all. Needs a Discord application id: create one at discord.com/developers/applications and paste it here. Until then Roséline stays quiet.")
        .build();

    let enabled = adw::SwitchRow::builder()
        .title("Show on Discord")
        .subtitle("While the game is running.")
        .active(settings.borrow().discord.enabled)
        .build();
    enabled.add_prefix(&row_prefix("user-available-symbolic"));
    enabled.connect_active_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().discord.enabled = row.is_active();
            let _ = settings.borrow().save();
        }
    ));
    group.add(&enabled);

    let app_id = adw::EntryRow::builder()
        .title("Application id")
        .text(&settings.borrow().discord.app_id)
        .build();
    app_id.add_prefix(&row_prefix("dialog-password-symbolic"));
    app_id.connect_changed(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().discord.app_id = row.text().to_string();
            let _ = settings.borrow().save();
        }
    ));
    group.add(&app_id);

    let details = adw::EntryRow::builder()
        .title("First line")
        .text(&settings.borrow().discord.details)
        .build();
    details.add_prefix(&row_prefix("dialog-information-symbolic"));
    details.set_tooltip_text(Some("Shown until a game is joined."));
    details.connect_changed(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().discord.details = row.text().to_string();
            let _ = settings.borrow().save();
        }
    ));
    group.add(&details);

    let join = adw::SwitchRow::builder()
        .title("Let people join")
        .subtitle("Puts a Join Game button on the presence that opens your server.")
        .active(settings.borrow().discord.join_button)
        .build();
    join.add_prefix(&row_prefix("system-users-symbolic"));
    join.connect_active_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().discord.join_button = row.is_active();
            let _ = settings.borrow().save();
        }
    ));
    group.add(&join);

    let state = adw::EntryRow::builder()
        .title("Second line")
        .text(&settings.borrow().discord.state)
        .build();
    state.add_prefix(&row_prefix("dialog-information-symbolic"));
    state.set_tooltip_text(Some("Shown until a game is joined."));
    state.connect_changed(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().discord.state = row.text().to_string();
            let _ = settings.borrow().save();
        }
    ));
    group.add(&state);

    page.add(&group);
    page
}
