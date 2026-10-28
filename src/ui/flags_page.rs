// SPDX-License-Identifier: GPL-3.0-or-later

//! fast flags: the allowlisted switches and a manual layer.
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
        .description("Allowlisted switches, written before every launch. Manual flags below are applied after them.")
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

    let manual = adw::PreferencesGroup::builder()
        .title("Manual flags")
        .build();

    let name = adw::EntryRow::builder().title("Flag name").build();
    name.add_prefix(&row_prefix("dialog-information-symbolic"));
    let value = adw::EntryRow::builder().title("Value").build();
    value.add_prefix(&row_prefix("dialog-information-symbolic"));
    let add = adw::ActionRow::builder()
        .title("Add a flag")
        .subtitle("Enter a name and value, then press Enter.")
        .build();
    let button = gtk::Button::builder()
        .label("Add")
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .sensitive(false)
        .build();
    add.add_suffix(&button);
    manual.add(&name);
    manual.add(&value);
    manual.add(&add);

    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));

    let update: Rc<dyn Fn()> = {
        let name = name.downgrade();
        let value = value.downgrade();
        let button = button.downgrade();
        Rc::new(move || {
            let (Some(name), Some(value), Some(button)) =
                (name.upgrade(), value.upgrade(), button.upgrade())
            else {
                return;
            };
            button.set_sensitive(
                valid_flag_name(name.text().trim()) && !value.text().trim().is_empty(),
            );
        })
    };

    let add_flag: Rc<dyn Fn()> = {
        let settings = settings.clone();
        let rows = rows.clone();
        let manual = manual.downgrade();
        let name = name.downgrade();
        let value = value.downgrade();
        let update = update.clone();
        Rc::new(move || {
            let (Some(manual), Some(name), Some(value)) =
                (manual.upgrade(), name.upgrade(), value.upgrade())
            else {
                return;
            };
            let key = name.text().trim().to_string();
            let text = value.text().trim().to_string();
            if !valid_flag_name(&key) || text.is_empty() {
                return;
            }
            settings.borrow_mut().user_flags.insert(key, text);
            let _ = settings.borrow().save();
            name.set_text("");
            value.set_text("");
            rebuild_manual(&settings, &manual, &rows);
            update();
            name.grab_focus();
        })
    };

    button.connect_clicked({
        let add_flag = add_flag.clone();
        move |_| add_flag()
    });
    name.connect_apply({
        let add_flag = add_flag.clone();
        move |_| add_flag()
    });
    value.connect_apply({
        let add_flag = add_flag.clone();
        move |_| add_flag()
    });
    name.connect_changed({
        let update = update.clone();
        move |_| update()
    });
    value.connect_changed({
        let update = update.clone();
        move |_| update()
    });

    rebuild_manual(&settings, &manual, &rows);
    update();
    page.add(&manual);
    page
}

fn valid_flag_name(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_alphabetic())
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn rebuild_manual(
    settings: &Rc<RefCell<Settings>>,
    group: &adw::PreferencesGroup,
    rows: &Rc<RefCell<Vec<adw::ActionRow>>>,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }
    let flags = settings.borrow().user_flags.clone();
    for (key, value) in flags {
        let row = adw::ActionRow::builder()
            .title(key.as_str())
            .subtitle(value.as_str())
            .build();
        row.add_prefix(&row_prefix("application-x-executable-symbolic"));
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Remove")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let group_weak = group.downgrade();
        let rows_weak = Rc::downgrade(rows);
        let settings = settings.clone();
        let key = key.clone();
        remove.connect_clicked(move |_| {
            settings.borrow_mut().user_flags.remove(&key);
            let _ = settings.borrow().save();
            if let (Some(group), Some(rows)) = (group_weak.upgrade(), rows_weak.upgrade()) {
                rebuild_manual(&settings, &group, &rows);
            }
        });
        row.add_suffix(&remove);
        group.add(&row);
        rows.borrow_mut().push(row);
    }
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
