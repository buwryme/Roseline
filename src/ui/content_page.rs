// SPDX-License-Identifier: GPL-3.0-or-later

//! the content page: two font choices, then the folder overlay.
//!
//! fonts are owned by the font picker; the folder overlay handles everything
//! else. skyboxes owns both `sky` roots; the platform category skips that
//! subtree.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{flat_button, icon_flat_button, open_folder_button, row_prefix};
use crate::config::Paths;
use crate::fonts;
use crate::launch;
use crate::overlay;
use crate::settings::Settings;
use crate::util::format_size;

type Refresh = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// title, bundle roots, row icon, and subtrees a sibling category owns.
const CATEGORIES: [(&str, &[&str], &str, &[&str]); 4] = [
    (
        "Cursors",
        &["Contents/Resources/content/textures/Cursors"],
        "input-mouse-symbolic",
        &[],
    ),
    (
        "Skyboxes",
        &[
            "Contents/Resources/PlatformContent/pc/textures/sky",
            "Contents/Resources/content/sky",
        ],
        "weather-clear-symbolic",
        &[],
    ),
    (
        "Sounds",
        &["Contents/Resources/content/sounds"],
        "audio-volume-high-symbolic",
        &[],
    ),
    (
        "Platform textures",
        &["Contents/Resources/PlatformContent/pc/textures"],
        "image-x-generic-symbolic",
        &["sky"],
    ),
];

pub fn build() -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    page.add(&fonts_group());

    let overlay_root = Paths::get().overlay();
    let refresh: Refresh = Rc::new(RefCell::new(None));
    let rows: Rc<RefCell<Vec<(Vec<String>, adw::ActionRow)>>> = Rc::new(RefCell::new(Vec::new()));

    let group = adw::PreferencesGroup::builder()
        .title("Overlay")
        .description("Files here replace the client's own before every launch, and again after updates. Copy fills a folder category with the client's current files, so you can start overriding with accurate file paths. Fonts live in the group above and are not part of this overlay.")
        .build();

    for (title, roots, icon, skip) in CATEGORIES {
        let row = adw::ActionRow::builder().title(title).build();
        row.add_prefix(&row_prefix(icon));

        let copy = flat_button("Copy");
        copy.set_icon_name("edit-copy-symbolic");
        copy.connect_clicked(glib::clone!(
            #[strong]
            refresh,
            move |button| {
                let (mut copied, mut kept) = (0, 0);
                for root in roots {
                    match overlay::seed_excluding(root, skip) {
                        Ok((c, k)) => {
                            copied += c;
                            kept += k;
                        }
                        Err(error) => eprintln!("roseline: could not seed {root}: {error:#}"),
                    }
                }
                if let Some(window) = button.root().and_downcast::<adw::ApplicationWindow>() {
                    super::window::toast(
                        &window,
                        &format!("{title}: {copied} copied, {kept} kept"),
                    );
                }
                if let Some(refresh) = refresh.borrow().as_ref() {
                    refresh();
                }
            }
        ));
        row.add_suffix(&copy);
        row.add_suffix(&open_folder_button(overlay_root.join(roots[0])));
        group.add(&row);
        rows.borrow_mut()
            .push((roots.iter().map(|root| root.to_string()).collect(), row));
    }

    let everything = adw::ActionRow::builder()
        .title("Copy everything")
        .subtitle("Runs Copy for all four folders at once.")
        .build();
    everything.add_prefix(&row_prefix("folder-open-symbolic"));
    let all = flat_button("Copy all");
    all.connect_clicked(glib::clone!(
        #[strong]
        refresh,
        move |button| {
            let (mut copied, mut kept) = (0, 0);
            for (_, roots, _, skip) in CATEGORIES {
                for root in roots {
                    if let Ok((c, k)) = overlay::seed_excluding(root, skip) {
                        copied += c;
                        kept += k;
                    }
                }
            }
            if let Some(window) = button.root().and_downcast::<adw::ApplicationWindow>() {
                super::window::toast(&window, &format!("{copied} copied, {kept} kept"));
            }
            if let Some(refresh) = refresh.borrow().as_ref() {
                refresh();
            }
        }
    ));
    everything.add_suffix(&all);
    everything.add_suffix(&open_folder_button(overlay_root));
    group.add(&everything);

    let clear_row = adw::ActionRow::builder()
        .title("Clear the overlay")
        .subtitle("Files already replaced stay until the client updates.")
        .build();
    clear_row.add_prefix(&row_prefix("user-trash-symbolic"));
    let clear = flat_button("Clear");
    clear.add_css_class("destructive-action");
    clear.connect_clicked(glib::clone!(
        #[strong]
        refresh,
        #[strong]
        rows,
        move |button| {
            let Some(window) = button.root().and_downcast::<adw::ApplicationWindow>() else {
                return;
            };
            let dialog = adw::AlertDialog::builder()
                .heading("Clear the overlay?")
                .body("Everything in the overlay folders, including old font copies, is removed. Files already replaced return after the client's next update. The font picks above are not changed.")
                .build();
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("clear", "Clear");
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            dialog.connect_response(
                None,
                glib::clone!(
                    #[strong]
                    refresh,
                    #[strong]
                    rows,
                    move |_, response| {
                        if response != "clear" {
                            return;
                        }
                        for (roots, _) in rows.borrow().iter() {
                            for root in roots {
                                let _ = overlay::clear(root);
                            }
                        }
                        let _ = overlay::clear(fonts::ROOT);
                        if let Some(refresh) = refresh.borrow().as_ref() {
                            refresh();
                        }
                    }
                ),
            );
            dialog.present(Some(&window));
        }
    ));
    clear_row.add_suffix(&clear);
    group.add(&clear_row);

    *refresh.borrow_mut() = Some({
        let rows = rows.clone();
        Rc::new(move || {
            for (roots, row) in rows.borrow().iter() {
                let (mut files, mut bytes) = (0, 0);
                for root in roots {
                    let (root_files, root_bytes) = overlay::measure(root);
                    files += root_files;
                    bytes += root_bytes;
                }
                row.set_subtitle(&match files {
                    0 => "Empty. Copy starts you off with the client's own files.".to_string(),
                    _ => format!("{files} files · {}", format_size(bytes)),
                });
            }
        })
    });
    if let Some(refresh) = refresh.borrow().as_ref() {
        refresh();
    }

    page.add(&group);
    page
}

fn fonts_group() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Fonts")
        .description("Replace fonts by category. Language-specific fonts and emoji are skipped.\nRoblox uses its own emoji format that isn't compatible with more widely used formats.")
        .build();

    let (font_row, font_choose, font_reset) = make_font_row(
        "Font",
        "font-select-symbolic",
        "Use the client's own text fonts",
    );
    let (mono_row, mono_choose, mono_reset) = make_font_row(
        "Monospace Font",
        "utilities-terminal-symbolic",
        "Use the client's own monospace fonts",
    );
    group.add(&font_row);
    group.add(&mono_row);

    let refresh: Rc<dyn Fn()> = {
        let font_row = font_row.downgrade();
        let mono_row = mono_row.downgrade();
        let font_reset = font_reset.downgrade();
        let mono_reset = mono_reset.downgrade();
        Rc::new(move || {
            let settings = Settings::load();
            let paths = Paths::get();
            if let (Some(row), Some(reset)) = (font_row.upgrade(), font_reset.upgrade()) {
                update_font_row(
                    &row,
                    &reset,
                    settings.fonts.font.as_deref(),
                    fonts::count(&paths, fonts::Kind::Text),
                    "the client's own text fonts",
                );
            }
            if let (Some(row), Some(reset)) = (mono_row.upgrade(), mono_reset.upgrade()) {
                update_font_row(
                    &row,
                    &reset,
                    settings.fonts.mono.as_deref(),
                    fonts::count(&paths, fonts::Kind::Mono),
                    "the client's own monospace fonts",
                );
            }
        })
    };
    refresh();

    connect_font_choice(&font_choose, fonts::Kind::Text, refresh.clone());
    connect_font_choice(&mono_choose, fonts::Kind::Mono, refresh.clone());
    connect_font_reset(&font_reset, fonts::Kind::Text, refresh.clone());
    connect_font_reset(&mono_reset, fonts::Kind::Mono, refresh);

    group
}

fn make_font_row(
    title: &str,
    icon: &str,
    reset_tooltip: &str,
) -> (adw::ActionRow, gtk::Button, gtk::Button) {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_prefix(&row_prefix(icon));
    let choose = flat_button("Choose…");
    let reset = icon_flat_button("edit-clear-symbolic", reset_tooltip);
    row.add_suffix(&choose);
    row.add_suffix(&reset);
    (row, choose, reset)
}

fn update_font_row(
    row: &adw::ActionRow,
    reset: &gtk::Button,
    choice: Option<&Path>,
    count: usize,
    empty: &str,
) {
    reset.set_sensitive(choice.is_some());
    let Some(path) = choice else {
        let subtitle = if count > 0 {
            format!("Using {empty} ({count} files).")
        } else {
            format!("Using {empty}. The client is not installed yet.")
        };
        row.set_subtitle(&subtitle);
        row.set_tooltip_text(None);
        return;
    };
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let subtitle = if !path.is_file() {
        format!("{name} is missing. Choose another file.")
    } else if count == 0 {
        format!("{name} · the client is not installed yet")
    } else {
        format!("{name} · replaces {count} files")
    };
    row.set_subtitle(&subtitle);
    row.set_tooltip_text(Some(&path.display().to_string()));
}

fn connect_font_choice(button: &gtk::Button, kind: fonts::Kind, refresh: Rc<dyn Fn()>) {
    button.connect_clicked(move |button| {
        let Some(window) = button.root().and_downcast::<adw::ApplicationWindow>() else {
            return;
        };
        let dialog = gtk::FileDialog::builder()
            .title(match kind {
                fonts::Kind::Text => "Choose a font",
                fonts::Kind::Mono => "Choose a monospace font",
            })
            .modal(true)
            .build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Font files"));
        for pattern in ["*.ttf", "*.otf", "*.TTF", "*.OTF"] {
            filter.add_pattern(pattern);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        dialog.set_default_filter(Some(&filter));
        if let Some(dir) = initial_font_dir(kind) {
            dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
        }

        let refresh = refresh.clone();
        let toast_window = window.clone();
        dialog.open(Some(&window), None::<&gio::Cancellable>, move |result| {
            let Ok(file) = result else {
                return;
            };
            let Some(path) = file.path() else {
                super::window::toast(&toast_window, "That font is not a local file.");
                return;
            };
            let imported = match fonts::import(&path, kind) {
                Ok(path) => path,
                Err(error) => {
                    super::window::toast(
                        &toast_window,
                        &format!("Could not use that font: {error:#}"),
                    );
                    return;
                }
            };
            let mut settings = Settings::load();
            match kind {
                fonts::Kind::Text => settings.fonts.font = Some(imported),
                fonts::Kind::Mono => settings.fonts.mono = Some(imported),
            }
            if let Err(error) = settings.save() {
                super::window::toast(
                    &toast_window,
                    &format!("Could not save the font: {error:#}"),
                );
                return;
            }

            let running = launch::running_pid().is_some();
            let applied = if running {
                Ok(fonts::Summary::default())
            } else {
                fonts::apply(&settings)
            };
            match applied {
                Ok(_) if running => {
                    super::window::toast(&toast_window, "Saved. It will apply at the next launch.")
                }
                Ok(summary) if summary.written > 0 => super::window::toast(
                    &toast_window,
                    &format!(
                        "Applied to {} files. It takes effect the next time you launch Roblox.",
                        summary.written
                    ),
                ),
                Ok(_) if Paths::get().app_bundle().is_dir() => {
                    super::window::toast(&toast_window, "Saved. It will apply at the next launch.")
                }
                Ok(_) => super::window::toast(
                    &toast_window,
                    "Saved. It will apply once the Roblox client is installed.",
                ),
                Err(error) => super::window::toast(
                    &toast_window,
                    &format!("Saved, but applying it failed: {error:#}"),
                ),
            }
            refresh();
        });
    });
}

fn connect_font_reset(button: &gtk::Button, kind: fonts::Kind, refresh: Rc<dyn Fn()>) {
    button.connect_clicked(move |button| {
        let Some(window) = button.root().and_downcast::<adw::ApplicationWindow>() else {
            return;
        };
        let mut settings = Settings::load();
        match kind {
            fonts::Kind::Text => settings.fonts.font = None,
            fonts::Kind::Mono => settings.fonts.mono = None,
        }
        if let Err(error) = settings.save() {
            super::window::toast(&window, &format!("Could not save the font: {error:#}"));
            return;
        }
        fonts::discard(kind);
        if launch::running_pid().is_some() {
            super::window::toast(
                &window,
                "Reset. The original fonts return at the next launch.",
            );
            refresh();
            return;
        }
        match fonts::apply(&settings) {
            Ok(summary) if summary.restored > 0 => {
                super::window::toast(&window, "The original fonts are back.")
            }
            Ok(_) => super::window::toast(&window, "Using the client's own fonts."),
            Err(error) => super::window::toast(
                &window,
                &format!("Reset, but restoring the files failed: {error:#}"),
            ),
        }
        refresh();
    });
}

fn initial_font_dir(kind: fonts::Kind) -> Option<PathBuf> {
    let settings = Settings::load();
    let current = match kind {
        fonts::Kind::Text => settings.fonts.font.as_deref(),
        fonts::Kind::Mono => settings.fonts.mono.as_deref(),
    };
    let store = Paths::get().font_store();
    current
        .and_then(Path::parent)
        .filter(|dir| dir.is_dir() && !dir.starts_with(&store))
        .map(Path::to_path_buf)
        .or_else(|| Some(glib::home_dir()))
}
