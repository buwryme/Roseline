// SPDX-License-Identifier: GPL-3.0-or-later

//! graphics API and device, two dropdowns.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::row_prefix;
use crate::gpu;
use crate::settings::{Backend, Settings};

pub fn build() -> adw::PreferencesPage {
    let settings = Rc::new(RefCell::new(Settings::load()));
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder().title("Graphics").build();
    group.add(&api_row(&settings));
    group.add(&device_row(&settings));
    page.add(&group);
    page
}

pub(crate) fn api_row(settings: &Rc<RefCell<Settings>>) -> adw::ComboRow {
    let api = adw::ComboRow::builder()
        .title("Graphics API")
        .subtitle("Choose the Graphics API Roblox will run under.\nAuto favours Vulkan.")
        .build();
    api.add_prefix(&row_prefix("video-display-symbolic"));
    let model = gtk::StringList::new(&[]);
    for backend in Backend::ALL {
        model.append(backend.label());
    }
    api.set_model(Some(&model));
    api.set_selected(match settings.borrow().graphics.backend {
        Backend::Auto => 0,
        Backend::Vulkan => 1,
        Backend::OpenGL => 2,
    });
    api.connect_selected_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            let backend = match row.selected() {
                1 => Backend::Vulkan,
                2 => Backend::OpenGL,
                _ => Backend::Auto,
            };
            settings.borrow_mut().graphics.backend = backend;
            let _ = settings.borrow().save();
        }
    ));
    api
}

pub(crate) fn device_row(settings: &Rc<RefCell<Settings>>) -> adw::ComboRow {
    let gpus = gpu::list();
    let device = adw::ComboRow::builder().title("Graphics Device").build();
    device.add_prefix(&row_prefix("video-display-symbolic"));
    let model = gtk::StringList::new(&["Automatic"]);
    for card in &gpus {
        model.append(&display_name(card, &gpus));
    }
    device.set_model(Some(&model));
    device.set_selected(match settings.borrow().graphics.device.as_deref() {
        Some(slot) => gpus
            .iter()
            .position(|card| card.slot == slot)
            .map_or(0, |index| index as u32 + 1),
        None => 0,
    });
    device.connect_selected_notify(glib::clone!(
        #[strong]
        settings,
        #[strong]
        gpus,
        move |row| {
            let slot = match row.selected() {
                0 => None,
                index => gpus.get(index as usize - 1).map(|card| card.slot.clone()),
            };
            settings.borrow_mut().graphics.device = slot;
            let _ = settings.borrow().save();
        }
    ));
    device
}

fn display_name(card: &gpu::Gpu, all: &[gpu::Gpu]) -> String {
    match all.iter().filter(|other| other.name == card.name).count() {
        1 => card.name.clone(),
        _ => format!("{} ({})", card.name, card.slot),
    }
}
