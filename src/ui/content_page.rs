// SPDX-License-Identifier: GPL-3.0-or-later

//! the content overlay, seeded from the client's own files.
//!
//! Skyboxes owns both `sky` roots; the platform category skips that subtree.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::{flat_button, open_folder_button, row_prefix};
use crate::config::Paths;
use crate::overlay;
use crate::util::format_size;

type Refresh = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// title, bundle roots, row icon, and subtrees a sibling category owns.
const CATEGORIES: [(&str, &[&str], &str, &[&str]); 5] = [
    (
        "Cursors",
        &["Contents/Resources/content/textures/Cursors"],
        "input-mouse-symbolic",
        &[],
    ),
    (
        "Fonts",
        &["Contents/Resources/content/fonts"],
        "font-select-symbolic",
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
    let overlay_root = Paths::get().overlay();

    let refresh: Refresh = Rc::new(RefCell::new(None));
    let rows: Rc<RefCell<Vec<(Vec<String>, adw::ActionRow)>>> = Rc::new(RefCell::new(Vec::new()));

    let group = adw::PreferencesGroup::builder()
        .title("Overlay")
        .description("Files here replace the client's own before every launch, and again after updates. Copy fills a folder category with the client's current files, so you can start overriding with accurate file paths. Overlaying is done safely.")
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
        .subtitle("Runs Copy for all five folders at once.")
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
        .subtitle("The client's own files come back at the next launch.")
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
                .body("Everything in the overlay folders is removed, and the client's own files return at the next launch.")
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
