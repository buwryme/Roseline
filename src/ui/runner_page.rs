// SPDX-License-Identifier: GPL-3.0-or-later

//! the Rosemary version, the Darwin base, and the prefix tools.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use anyhow::Context;
use gtk::glib;

use super::row_prefix;
use super::window::toast;
use crate::config::Paths;
use crate::settings::{Settings, SystemFiles};

#[derive(Clone)]
struct Entry {
    tag: String,
    /// installed runner id, when this version is on disk.
    id: Option<String>,
    custom: bool,
}

fn local_entries() -> Vec<Entry> {
    let customs = crate::runner::customs()
        .into_iter()
        .map(|(id, name)| Entry {
            tag: name,
            id: Some(id),
            custom: true,
        });
    crate::runner::installed()
        .into_iter()
        .map(|(id, tag)| Entry {
            tag,
            id: Some(id),
            custom: false,
        })
        .chain(customs)
        .collect()
}

/// pick a folder through the portal, then name it; confirming copies the
/// build in and pins it.
fn choose_folder_then_name(
    window: &adw::ApplicationWindow,
    settings: &Rc<RefCell<Settings>>,
    entries: &Rc<RefCell<Vec<Entry>>>,
    rebuild: &Rc<dyn Fn()>,
) {
    let dialog = gtk::FileDialog::builder()
        .title("Choose a Rosemary build")
        .modal(true)
        .build();
    let window_for_result = window.clone();
    let settings = settings.clone();
    let entries = entries.clone();
    let rebuild = rebuild.clone();
    dialog.select_folder(
        Some(window),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            let Ok(file) = result else {
                return;
            };
            let Some(source) = file.path() else {
                toast(&window_for_result, "That folder is not a local directory.");
                return;
            };
            let Ok(real) = source.canonicalize() else {
                toast(&window_for_result, "That folder does not exist.");
                return;
            };
            ask_name_and_import(&window_for_result, real, &settings, &entries, &rebuild);
        },
    );
}

fn ask_name_and_import(
    window: &adw::ApplicationWindow,
    source: std::path::PathBuf,
    settings: &Rc<RefCell<Settings>>,
    entries: &Rc<RefCell<Vec<Entry>>>,
    rebuild: &Rc<dyn Fn()>,
) {
    let entry = gtk::Entry::builder()
        .placeholder_text("My Rosemary build")
        .activates_default(true)
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading("Name this Rosemary build")
        .extra_child(&entry)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", "Confirm");
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("confirm"));
    dialog.set_close_response("cancel");
    let validate = {
        let dialog = dialog.clone();
        let entry = entry.clone();
        move || {
            let name = entry.text().chars().collect::<String>();
            let found = crate::runner::name_taken(&name);
            let clean = !found && !name.is_empty() && !name.contains('/');
            dialog.set_response_enabled("confirm", clean);
        }
    };
    entry.connect_changed(glib::clone!(
        #[strong]
        validate,
        move |_| validate()
    ));

    let weak_window = window.downgrade();
    let settings = settings.clone();
    let entries = entries.clone();
    let rebuild = rebuild.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "confirm" {
            return;
        }
        let name = entry.text().trim().to_string();
        if name.is_empty() || crate::runner::name_taken(&name) {
            return;
        }
        if let Some(window) = weak_window.upgrade() {
            toast(&window, &format!("Importing {name}…"));
        }
        let source = source.clone();
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = crate::runner::import_custom(&source, &name);
            let _ = sender.send_blocking((name, result));
        });
        glib::spawn_future_local(glib::clone!(
            #[strong]
            settings,
            #[strong]
            entries,
            #[strong]
            rebuild,
            #[strong]
            weak_window,
            async move {
                let Ok((name, result)) = receiver.recv().await else {
                    return;
                };
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                match result {
                    Ok(id) => {
                        settings.borrow_mut().runner = Some(id);
                        let _ = settings.borrow().save();
                        entries.borrow_mut().push(Entry {
                            tag: name,
                            id: settings.borrow().runner.clone(),
                            custom: true,
                        });
                        rebuild();
                        toast(&window, "Import finished");
                    }
                    Err(error) => toast(&window, &format!("Couldn't import {name}: {error:#}")),
                }
            }
        ));
    });
    dialog.present(Some(window));
}

pub fn build() -> gtk::Widget {
    let page = adw::PreferencesPage::new();
    let paths = Paths::get();
    let settings = Rc::new(RefCell::new(Settings::load()));

    let group = adw::PreferencesGroup::builder()
        .title("Rosemary")
        .description(
            "Changing versions into an older one may corrupt your prefix. Applied on next launch.",
        )
        .build();

    let entries: Rc<RefCell<Vec<Entry>>> = Rc::new(RefCell::new(local_entries()));
    let loading = Rc::new(Cell::new(false));
    let combo = adw::ComboRow::builder().title("Version").build();
    combo.add_prefix(&row_prefix("system-run-symbolic"));

    let rebuild: Rc<dyn Fn()> = {
        let combo = combo.clone();
        let entries = entries.clone();
        let settings = settings.clone();
        let loading = loading.clone();
        Rc::new(move || {
            loading.set(true);
            let model = gtk::StringList::new(&[]);
            let pinned = settings.borrow().runner.clone();
            let mut selected = 0u32;
            let mut newest_installed = None;
            for (index, entry) in entries.borrow().iter().enumerate() {
                model.append(&match (&entry.id, entry.custom) {
                    (Some(_), true) => format!("{} (custom)", entry.tag),
                    (Some(_), false) => format!("{} (installed)", entry.tag),
                    (None, _) => entry.tag.clone(),
                });
                if entry.id.is_some() && newest_installed.is_none() {
                    newest_installed = Some(index as u32);
                }
                if pinned.as_deref() == entry.id.as_deref() {
                    selected = index as u32;
                }
            }
            if pinned.is_none()
                && let Some(index) = newest_installed
            {
                selected = index;
            }
            if model.n_items() == 0 {
                model.append("No versions found yet");
            }
            combo.set_model(Some(&model));
            combo.set_selected(selected);
            loading.set(false);
        })
    };
    rebuild();

    combo.connect_selected_notify(glib::clone!(
        #[strong]
        settings,
        #[strong]
        entries,
        #[strong]
        loading,
        #[strong]
        rebuild,
        move |row| {
            if loading.get() {
                return;
            }
            let index = row.selected() as usize;
            let Some(entry) = entries.borrow().get(index).cloned() else {
                return;
            };
            if let Some(id) = &entry.id {
                settings.borrow_mut().runner = Some(id.clone());
                let _ = settings.borrow().save();
                return;
            }
            let Some(window) = row.root().and_downcast::<adw::ApplicationWindow>() else {
                return;
            };
            let weak_window = window.downgrade();
            let tag = entry.tag.clone();
            toast(&window, &format!("Downloading Rosemary {tag}…"));
            let (sender, receiver) = async_channel::bounded(1);
            let tag_for_fetch = tag.clone();
            std::thread::spawn(move || {
                let result = crate::github::releases().and_then(|list| {
                    let release = list
                        .into_iter()
                        .find(|release| release.tag_name == tag_for_fetch)
                        .context("this release vanished from GitHub")?;
                    crate::runner::install(&release, &crate::progress::Progress::silent())
                });
                let _ = sender.send_blocking(result);
            });
            glib::spawn_future_local(glib::clone!(
                #[strong]
                settings,
                #[strong]
                entries,
                #[strong]
                rebuild,
                async move {
                    match receiver.recv().await {
                        Ok(Ok(id)) => {
                            settings.borrow_mut().runner = Some(id.clone());
                            let _ = settings.borrow().save();
                            {
                                let mut entries = entries.borrow_mut();
                                if let Some(entry) =
                                    entries.iter_mut().find(|entry| entry.tag == tag)
                                {
                                    entry.id = Some(id);
                                }
                            }
                            rebuild();
                            if let Some(window) = weak_window.upgrade() {
                                toast(&window, &format!("Rosemary {tag} installed"));
                            }
                        }
                        Ok(Err(error)) => {
                            eprintln!("roseline: could not install {tag}: {error:#}");
                            if let Some(window) = weak_window.upgrade() {
                                toast(
                                    &window,
                                    &format!("Couldn't install {tag}; see the terminal"),
                                );
                            }
                        }
                        Err(_) => {}
                    }
                }
            ));
        }
    ));
    group.add(&combo);

    let import_row = adw::ActionRow::builder()
        .title("Import a custom build")
        .subtitle("Use a downloaded or self-built Rosemary; it is pinned and official updates leave it alone.")
        .build();
    import_row.add_prefix(&row_prefix("archive-insert-symbolic"));
    let import_button = super::flat_button("Import");
    import_button.connect_clicked(glib::clone!(
        #[strong]
        settings,
        #[strong]
        entries,
        #[strong]
        rebuild,
        move |button| {
            let Some(window) = button.root().and_downcast::<adw::ApplicationWindow>() else {
                return;
            };
            choose_folder_then_name(&window, &settings, &entries, &rebuild);
        }
    ));
    import_row.add_suffix(&import_button);
    import_row.set_activatable_widget(Some(&import_button));
    group.add(&import_row);
    page.add(&group);

    if cfg!(target_arch = "aarch64") {
        let compatibility = adw::PreferencesGroup::builder()
            .title("ARM64 compatibility")
            .build();
        let toggle = adw::SwitchRow::builder()
            .title("Use muvm for x86_64 runners")
            .subtitle("Runs x86_64 Rosemary through muvm and FEX. Disable to require a native ARM64 runner. Applied on next launch.")
            .active(settings.borrow().compatibility.muvm)
            .build();
        let toggle_settings = settings.clone();
        toggle.connect_active_notify(move |toggle| {
            toggle_settings.borrow_mut().compatibility.muvm = toggle.is_active();
            let _ = toggle_settings.borrow().save();
        });
        compatibility.add(&toggle);

        let labels: Vec<_> = crate::settings::VmMemory::ALL
            .iter()
            .map(|memory| memory.label())
            .collect();
        let memory = adw::ComboRow::builder()
            .title("VM memory")
            .subtitle("Guest RAM only; graphics quality is unchanged. Choose muvm default to use its normal memory sizing, or increase it for games needing more memory.")
            .model(&gtk::StringList::new(&labels))
            .selected(crate::settings::VmMemory::ALL.iter()
                .position(|value| *value == settings.borrow().compatibility.vm_memory)
                .unwrap_or(0) as u32)
            .build();
        let memory_settings = settings.clone();
        memory.connect_selected_notify(move |row| {
            if let Some(value) = crate::settings::VmMemory::ALL.get(row.selected() as usize) {
                memory_settings.borrow_mut().compatibility.vm_memory = *value;
                let _ = memory_settings.borrow().save();
            }
        });
        compatibility.add(&memory);
        page.add(&compatibility);
    }

    // versions this machine lacks appear once GitHub answers.
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = sender.send_blocking(crate::github::releases().map(|list| {
            list.into_iter()
                .map(|release| release.tag_name)
                .collect::<Vec<_>>()
        }));
    });
    glib::spawn_future_local(glib::clone!(
        #[strong]
        entries,
        #[strong]
        rebuild,
        async move {
            let Ok(Ok(tags)) = receiver.recv().await else {
                return;
            };
            {
                let mut entries = entries.borrow_mut();
                for tag in tags {
                    if !entries.iter().any(|entry| entry.tag == tag) {
                        entries.push(Entry {
                            tag,
                            id: None,
                            custom: false,
                        });
                    }
                }
                entries.sort_by(|a, b| {
                    a.custom.cmp(&b.custom).then_with(|| {
                        crate::runner::version_key(&b.tag).cmp(&crate::runner::version_key(&a.tag))
                    })
                });
            }
            rebuild();
        }
    ));

    let base_group = adw::PreferencesGroup::builder()
        .title("MiniDarwin base")
        .description("The Darwin userland in the prefix, downloaded hash-checked on first use. This policy decides what happens when a program writes a system file.")
        .build();
    let mut radios: Vec<gtk::CheckButton> = Vec::new();
    for policy in SystemFiles::ALL {
        let row = adw::ActionRow::builder()
            .title(policy.label())
            .subtitle(policy.description())
            .build();
        row.add_prefix(&row_prefix(policy.icon()));
        let check = gtk::CheckButton::new();
        if let Some(first) = radios.first() {
            check.set_group(Some(first));
        }
        radios.push(check.clone());
        check.set_active(settings.borrow().prefix.system_files == policy);
        row.add_suffix(&check);
        row.set_activatable_widget(Some(&check));
        let settings = settings.clone();
        check.connect_toggled(move |check| {
            if !check.is_active() {
                return;
            }
            settings.borrow_mut().prefix.system_files = policy;
            let _ = settings.borrow().save();
        });
        base_group.add(&row);
    }
    page.add(&base_group);

    let runner_installed = paths.runner().is_some();
    let maintenance = adw::PreferencesGroup::builder()
        .title("Maintenance")
        .build();
    for (action, title, subtitle, icon) in [
        (
            "verify",
            "Verify files",
            "Checks every system file against the base's manifest.",
            "system-search-symbolic",
        ),
        (
            "repair",
            "Repair",
            "Puts back what differs, offline.",
            "view-refresh-symbolic",
        ),
        (
            "reset-system",
            "Reset system files",
            "Also moves aside what a user put in a system directory.",
            "user-trash-symbolic",
        ),
    ] {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .build();
        row.add_prefix(&row_prefix(icon));
        let button = super::flat_button("Run");
        button.set_sensitive(runner_installed);
        button.connect_clicked(move |button| run_action(button, action, title));
        row.add_suffix(&button);
        row.set_activatable_widget(Some(&button));
        maintenance.add(&row);
    }
    let logs = adw::ActionRow::builder().title("Logs").build();
    logs.add_prefix(&row_prefix("text-x-generic-symbolic"));
    logs.add_suffix(&super::open_folder_button(paths.logs()));
    maintenance.add(&logs);
    page.add(&maintenance);

    page.add(&environment_group(
        &settings,
        "Mac program environment",
        "Passed to the Mac programs Rosemary runs, as ROSEMARY_ENVIRON. NAME=VALUE, or a bare NAME to copy this computer's value.",
        |settings| &mut settings.environment.guest,
    ));
    page.add(&environment_group(
        &settings,
        "Rosemary environment",
        "Set for Rosemary's own process: drivers, tuning, and the like. NAME=VALUE, or a bare NAME to copy this computer's value.",
        |settings| &mut settings.environment.runner,
    ));

    let open_config = gtk::Button::builder()
        .label("Open Config File")
        .halign(gtk::Align::Center)
        .css_classes(["suggested-action", "pill"])
        .build();
    open_config.connect_clicked(|button| {
        let path = Paths::get().settings_file();
        if !path.exists() {
            // first launch seeds the template; the button still opens it.
            let _ = Settings::load();
        }
        let launcher = gtk::FileLauncher::new(Some(&gtk::gio::File::for_path(&path)));
        let window = button.root().and_downcast::<adw::ApplicationWindow>();
        launcher.launch(window.as_ref(), gtk::gio::Cancellable::NONE, |result| {
            if let Err(error) = result {
                eprintln!("roseline: could not open the config file: {error}");
            }
        });
    });

    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .margin_top(12)
        .margin_bottom(18)
        .margin_start(24)
        .margin_end(24)
        .build();
    footer.append(&open_config);

    page.set_vexpand(true);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&page);
    root.append(&footer);
    root.upcast()
}

fn run_action(button: &gtk::Button, action: &'static str, title: &'static str) {
    let Some(window) = button.root().and_downcast::<adw::ApplicationWindow>() else {
        return;
    };
    toast(&window, &format!("{title}…"));
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let log = Paths::get().logs().join(format!("prefix-{action}.log"));
        let result = crate::rosemary::prefix(action, &log);
        let _ = sender.send_blocking(result);
    });
    glib::spawn_future_local(async move {
        let text = match receiver.recv().await {
            Ok(Ok(status)) if status.success() => format!("{title}: done."),
            _ => format!("{title}: failed. The log is in the bottle's logs folder."),
        };
        toast(&window, &text);
    });
}

/// one environment list; `pick` decides which list the page touches.
pub(crate) fn environment_group(
    settings: &Rc<RefCell<Settings>>,
    title: &'static str,
    description: &'static str,
    pick: fn(&mut Settings) -> &mut Vec<String>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(title)
        .description(description)
        .build();
    let inserted: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));

    let add_row = adw::EntryRow::builder()
        .title("Add variable")
        .show_apply_button(true)
        .build();
    add_row.add_prefix(&row_prefix("list-add-symbolic"));

    let fill_slot: Rc<RefCell<Option<std::rc::Weak<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let fill: Rc<dyn Fn()> = {
        let group = group.clone();
        let inserted = inserted.clone();
        let settings = settings.clone();
        let add_row_weak = add_row.downgrade();
        let fill_slot = fill_slot.clone();
        Rc::new(move || {
            for widget in inserted.borrow_mut().drain(..) {
                group.remove(&widget);
            }
            let lines = pick(&mut settings.borrow_mut()).clone();
            for line in lines {
                let row = adw::ActionRow::builder().title(line.clone()).build();
                let edit = super::icon_flat_button("document-edit-symbolic", "Edit");
                let remove = super::icon_flat_button("user-trash-symbolic", "Remove");

                let edit_fill = fill_slot.clone();
                let edit_settings = settings.clone();
                let edit_line = line.clone();
                edit.connect_clicked(move |button| {
                    let Some(fill) = edit_fill.borrow().as_ref().and_then(std::rc::Weak::upgrade)
                    else {
                        return;
                    };
                    let Some(window) = button.root().and_downcast::<adw::ApplicationWindow>()
                    else {
                        return;
                    };
                    let settings = edit_settings.clone();
                    let line = edit_line.clone();
                    let save_line = line.clone();
                    edit_env_dialog(&window, &line, move |new_line| {
                        {
                            let mut settings = settings.borrow_mut();
                            if let Some(slot) = pick(&mut settings)
                                .iter_mut()
                                .find(|entry| **entry == save_line)
                            {
                                *slot = new_line;
                            }
                        }
                        let _ = settings.borrow().save();
                        fill();
                    });
                });

                let remove_fill = fill_slot.clone();
                let remove_settings = settings.clone();
                remove.connect_clicked(move |_| {
                    pick(&mut remove_settings.borrow_mut()).retain(|entry| *entry != line);
                    let _ = remove_settings.borrow().save();
                    if let Some(fill) = remove_fill
                        .borrow()
                        .as_ref()
                        .and_then(std::rc::Weak::upgrade)
                    {
                        fill();
                    }
                });

                row.add_suffix(&edit);
                row.add_suffix(&remove);
                group.add(&row);
                inserted.borrow_mut().push(row.upcast());
            }
            if let Some(add_row) = add_row_weak.upgrade() {
                group.add(&add_row);
                inserted.borrow_mut().push(add_row.upcast());
            }
        })
    };
    *fill_slot.borrow_mut() = Some(Rc::downgrade(&fill));

    add_row.connect_apply(glib::clone!(
        #[strong]
        settings,
        move |row| {
            let line = row.text().trim().to_string();
            if !crate::launch::valid_env_line(&line) {
                if let Some(window) = row.root().and_downcast::<adw::ApplicationWindow>() {
                    toast(&window, "Use NAME=VALUE, or a bare NAME");
                }
                return;
            }
            pick(&mut settings.borrow_mut()).push(line);
            let _ = settings.borrow().save();
            row.set_text("");
            if let Some(fill) = fill_slot.borrow().as_ref().and_then(std::rc::Weak::upgrade) {
                fill();
            }
        }
    ));

    fill();
    // the group owns the closures; when the page goes, they go.
    unsafe {
        group.set_data("roseline-env-fill", fill);
        group.set_data("roseline-env-add", add_row);
    }
    group
}

/// a small dialog for one line, so editing does not fight the list.
fn edit_env_dialog(
    window: &adw::ApplicationWindow,
    current: &str,
    save: impl Fn(String) + 'static,
) {
    let entry = adw::EntryRow::builder()
        .title("NAME=VALUE")
        .text(current)
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading("Edit variable")
        .extra_child(&entry)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    dialog.connect_response(
        None,
        glib::clone!(
            #[weak]
            entry,
            move |dialog, response| {
                if response != "save" {
                    return;
                }
                let line = entry.text().trim().to_string();
                if crate::launch::valid_env_line(&line) {
                    save(line);
                } else if let Some(window) = dialog.root().and_downcast::<adw::ApplicationWindow>()
                {
                    toast(&window, "Use NAME=VALUE, or a bare NAME");
                }
            }
        ),
    );
    dialog.present(Some(window));
}
