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

    let show = adw::SwitchRow::builder()
        .title("Show Game")
        .subtitle("Show the game you're in. Off only shows that Roblox is open.")
        .active(settings.borrow().discord.show_game)
        .build();
    show.add_prefix(&row_prefix("applications-games-symbolic"));

    let join = adw::SwitchRow::builder()
        .title("Let People Join")
        .subtitle(
            "Adds a Join Game button on public servers, plus a View Game link. Join opens in the browser, so Flatpak Discord can hand it off. Private servers only get View Game.",
        )
        .active(settings.borrow().discord.join_button)
        .sensitive(settings.borrow().discord.show_game)
        .build();
    join.add_prefix(&row_prefix("system-users-symbolic"));

    show.connect_active_notify(glib::clone!(
        #[strong]
        settings,
        #[strong]
        join,
        move |row| {
            settings.borrow_mut().discord.show_game = row.is_active();
            let _ = settings.borrow().save();
            join.set_sensitive(row.is_active());
            crate::ui::splash::refresh_presence();
        }
    ));
    join.connect_active_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().discord.join_button = row.is_active();
            let _ = settings.borrow().save();
            crate::ui::splash::refresh_presence();
        }
    ));
    group.add(&show);
    group.add(&join);

    let avatar = adw::SwitchRow::builder()
        .title("Replace Roblox small image with your avatar head")
        .active(settings.borrow().discord.show_avatar)
        .build();
    avatar.add_prefix(&row_prefix("avatar-default-symbolic"));
    avatar.connect_active_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().discord.show_avatar = row.is_active();
            let _ = settings.borrow().save();
            crate::ui::splash::refresh_presence();
        }
    ));
    group.add(&avatar);

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

    let player = adw::PreferencesGroup::builder().title("Player").build();
    let close = adw::SwitchRow::builder()
        .title("Close when leaving")
        .subtitle(
            "Closes Roblox when you leave an experience, instead of returning to the home screen.",
        )
        .active(settings.borrow().behavior.close_on_leave)
        .build();
    close.add_prefix(&row_prefix("application-exit-symbolic"));
    close.connect_active_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().behavior.close_on_leave = row.is_active();
            let _ = settings.borrow().save();
        }
    ));
    player.add(&close);
    page.add(&player);

    let server = adw::PreferencesGroup::builder()
        .title("Server Location")
        .description("Sends a notification with the server's city and country after joining. Uses the server IP from Roblox and ipwho.is.")
        .build();
    let toast = adw::SwitchRow::builder()
        .title("Show server location")
        .subtitle("Sends a notification with the server's city and country after joining.")
        .active(settings.borrow().server_location.enabled)
        .build();
    toast.add_prefix(&row_prefix("mark-location-symbolic"));
    toast.connect_active_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().server_location.enabled = row.is_active();
            let _ = settings.borrow().save();
        }
    ));
    server.add(&toast);
    page.add(&server);
    page
}
