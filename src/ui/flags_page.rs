// SPDX-License-Identifier: GPL-3.0-or-later

//! fast flags, locked to Roblox's allowlist.
//!
//! slider ranges follow the community's documentation: texture 0-3, CSG LOD
//! distances 0-1000 studs.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::{open_uri_button, row_prefix};
use crate::flags;
use crate::settings::Settings;

pub fn build() -> adw::PreferencesPage {
    let settings = Rc::new(RefCell::new(Settings::load()));
    let page = adw::PreferencesPage::new();

    let group = adw::PreferencesGroup::builder()
        .title("FFlags")
        .description("The client ignores any flags not on Roblox's allowlist, listed below. Written before every launch.")
        .build();

    let allowlist = adw::ActionRow::builder()
        .title("Roblox's allowlist")
        .subtitle("The official list, on the Roblox developer forum.")
        .build();
    allowlist.add_prefix(&row_prefix("applications-internet-symbolic"));
    allowlist.add_suffix(&open_uri_button("Read", flags::ALLOWLIST_URL));
    group.add(&allowlist);

    group.add(&int_expander(
        "Render distance",
        "How far detailed geometry is kept, in studs. Sets the four CSG LOD distances; 0 forces the lowest detail.",
        "image-x-generic-symbolic",
        1000.0,
        25.0,
        1000.0,
        &settings,
        Rc::new(|settings| {
            settings
                .flags
                .get("DFIntCSGLevelOfDetailSwitchingDistance")
                .and_then(|value| value.parse().ok())
        }),
        Rc::new(|settings, value| {
            for key in [
                "DFIntCSGLevelOfDetailSwitchingDistance",
                "DFIntCSGLevelOfDetailSwitchingDistanceL12",
                "DFIntCSGLevelOfDetailSwitchingDistanceL23",
                "DFIntCSGLevelOfDetailSwitchingDistanceL34",
            ] {
                set_flag(settings, key, value);
            }
        }),
    ));

    group.add(&int_expander(
        "Texture quality",
        "0 is the lowest, 3 the highest. High values can exhaust VRAM on smaller cards.",
        "view-grid-symbolic",
        3.0,
        1.0,
        3.0,
        &settings,
        Rc::new(|settings| {
            settings
                .flags
                .get("DFIntTextureQualityOverride")
                .and_then(|value| value.parse().ok())
        }),
        Rc::new(|settings, value| {
            set_flag(settings, "DFIntTextureQualityOverride", value);
            set_flag(
                settings,
                "DFFlagTextureQualityOverrideEnabled",
                value.map(|_| 1),
            );
        }),
    ));

    for preset in &flags::PRESETS {
        let row = adw::ComboRow::builder()
            .title(preset.label)
            .subtitle(preset.description)
            .build();
        row.add_prefix(&row_prefix(preset.icon));

        let model = gtk::StringList::new(&[]);
        for choice in preset.choices {
            model.append(choice.label);
        }
        row.set_model(Some(&model));

        let current = settings.borrow().flags.get(preset.key).cloned();
        row.set_selected(
            preset
                .choices
                .iter()
                .position(|choice| choice.value.map(str::to_string) == current)
                .unwrap_or(0) as u32,
        );

        let settings = settings.clone();
        row.connect_selected_notify(move |row| {
            let Some(choice) = preset.choices.get(row.selected() as usize) else {
                return;
            };
            {
                let mut settings = settings.borrow_mut();
                match choice.value {
                    Some(value) => {
                        settings
                            .flags
                            .insert(preset.key.to_string(), value.to_string());
                    }
                    None => {
                        settings.flags.remove(preset.key);
                    }
                }
            }
            let _ = settings.borrow().save();
        });
        group.add(&row);
    }

    page.add(&group);
    page
}

fn set_flag(settings: &mut Settings, key: &str, value: Option<i64>) {
    match value {
        Some(value) => {
            settings.flags.insert(key.to_string(), value.to_string());
        }
        None => {
            settings.flags.remove(key);
        }
    }
}

/// expander with one integer slider; the switch means the flag is set at all.
fn int_expander(
    title: &'static str,
    subtitle: &'static str,
    icon: &'static str,
    max: f64,
    step: f64,
    default: f64,
    settings: &Rc<RefCell<Settings>>,
    read: Rc<dyn Fn(&Settings) -> Option<i64>>,
    write: Rc<dyn Fn(&mut Settings, Option<i64>)>,
) -> adw::ExpanderRow {
    let current = read(&settings.borrow());
    let expander = adw::ExpanderRow::builder()
        .title(title)
        .subtitle(subtitle)
        .show_enable_switch(true)
        .enable_expansion(current.is_some())
        .build();
    expander.add_prefix(&row_prefix(icon));

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, max, step);
    scale.set_draw_value(false);
    scale.set_width_request(220);
    scale.set_valign(gtk::Align::Center);
    scale.set_value(
        current
            .map(|value| value as f64)
            .unwrap_or(default)
            .clamp(0.0, max),
    );
    let label = gtk::Label::builder()
        .width_request(44)
        .css_classes(["dim-label"])
        .build();
    label.set_text(&format!("{}", scale.value() as i64));

    let level = adw::ActionRow::builder().title("Level").build();
    level.add_suffix(&label);
    level.add_suffix(&scale);
    level.set_activatable_widget(Some(&scale));
    expander.add_row(&level);

    scale.connect_value_changed(glib::clone!(
        #[strong]
        settings,
        #[strong]
        write,
        #[weak]
        expander,
        #[weak]
        label,
        move |scale| {
            let value = scale.value() as i64;
            label.set_text(&value.to_string());
            if expander.enables_expansion() {
                write(&mut settings.borrow_mut(), Some(value));
                let _ = settings.borrow().save();
            }
        }
    ));
    expander.connect_enable_expansion_notify(glib::clone!(
        #[strong]
        settings,
        #[strong]
        write,
        #[weak]
        scale,
        move |expander| {
            let value = expander.enables_expansion().then(|| scale.value() as i64);
            write(&mut settings.borrow_mut(), value);
            let _ = settings.borrow().save();
        }
    ));
    expander
}
