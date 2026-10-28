// SPDX-License-Identifier: GPL-3.0-or-later

mod avatar;
mod background;
mod config;
mod cookies;
mod flags;
mod fonts;
mod gameinfo;
mod github;
mod gpu;
mod icons;
mod launch;
mod muvm;
mod net;
mod overlay;
mod progress;
mod roblox;
mod rosemary;
mod rpc;
mod runner;
mod server_toast;
mod session;
mod settings;
mod ui;
mod util;

use adw::prelude::*;
use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(index) = arguments
        .iter()
        .position(|argument| argument == "--toast-helper")
    {
        let log = arguments.get(index + 1).map(std::path::PathBuf::from);
        let pid = arguments
            .get(index + 2)
            .and_then(|value| value.parse::<u32>().ok());
        if let (Some(log), Some(pid)) = (log, pid) {
            server_toast::helper(log, pid);
        }
        return glib::ExitCode::SUCCESS;
    }
    glib::set_application_name(config::DISPLAY_NAME);

    let app = adw::Application::builder()
        .application_id(config::APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    app.connect_startup(|app| {
        icons::init();
        gtk::Window::set_default_icon_name(config::APP_ID);
        let about = gio::ActionEntry::builder("about")
            .activate(|app: &adw::Application, _, _| ui::window::about(app))
            .build();
        let quit = gio::ActionEntry::builder("quit")
            .activate(|app: &adw::Application, _, _| app.quit())
            .build();
        app.add_action_entries([about, quit]);
        app.set_accels_for_action("app.quit", &["<Primary>q"]);
    });

    // a second launch lands in the running instance; the entries pass `%U` links in.
    app.connect_command_line(|app, command_line| {
        let arguments: Vec<String> = command_line
            .arguments()
            .iter()
            .skip(1)
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        let mut wants_settings = false;
        let mut settings_page = None;
        let mut uri = None;
        let mut remaining = arguments.into_iter();
        while let Some(argument) = remaining.next() {
            match argument.as_str() {
                "--settings" => {
                    wants_settings = true;
                    let next = remaining.next();
                    settings_page = match next.as_deref() {
                        Some("graphics" | "content" | "fflags" | "extras" | "runner") => next,
                        other => {
                            if let Some(other) = other
                                && looks_like_link(other)
                            {
                                uri = Some(other.to_string());
                            }
                            None
                        }
                    };
                }
                "--run" => uri = remaining.next(),
                _ if looks_like_link(&argument) => uri = Some(argument),
                _ => {}
            }
        }
        if wants_settings {
            ui::window::request_settings(app, settings_page.as_deref());
        } else {
            ui::splash::start(uri);
        }
        glib::ExitCode::SUCCESS
    });

    app.run()
}

/// `roblox://` and `roblox-player:` links, as the desktop entries pass them.
fn looks_like_link(argument: &str) -> bool {
    let Some((scheme, _)) = argument.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && !scheme.eq_ignore_ascii_case("file")
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}
