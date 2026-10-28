// SPDX-License-Identifier: GPL-3.0-or-later

//! first-run setup: the one-time replacement for the splash.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::row_prefix;
use super::runner_page;
use crate::config::Paths;
use crate::launch;
use crate::progress::{Event, Progress};
use crate::settings::Settings;

const LOGO_RESOURCE: &str = "/net/buwryy/Roseline/icons/scalable/apps/net.buwryy.Roseline.svg";
const MARKER: &str = "setup-done";

thread_local! {
    static SETUP: RefCell<Option<Rc<Setup>>> = const { RefCell::new(None) };
}

/// whether the first-run setup already ran to completion.
pub fn is_complete() -> bool {
    Paths::get().data.join(MARKER).is_file()
}

/// show the setup window, reusing it when open.
pub fn start(uri: Option<String>) {
    if let Some(setup) = SETUP.with(|slot| slot.borrow().clone()) {
        setup.window.present();
        return;
    }
    let setup = Setup::new(uri);
    SETUP.with(|slot| *slot.borrow_mut() = Some(setup.clone()));
    setup.window.present();
}

struct Setup {
    window: adw::ApplicationWindow,
    launch_check: gtk::CheckButton,
    completed: Rc<Cell<bool>>,
    finishing: Cell<bool>,
    uri: Option<String>,
}

impl Setup {
    fn new(uri: Option<String>) -> Rc<Setup> {
        let settings = Rc::new(RefCell::new(Settings::load()));
        let selected = Rc::new(RefCell::new(None));
        let completed = Rc::new(Cell::new(false));
        let launch_check = gtk::CheckButton::builder()
            .label("Launch Roséline once I close this window")
            .active(true)
            .halign(gtk::Align::Center)
            .build();

        let nav = adw::NavigationView::new();
        let (fetch, start_fetch, done) =
            fetch_page(selected.clone(), completed.clone(), launch_check.clone());
        let root = root_page(&nav, settings, selected, fetch, start_fetch);
        nav.push(&root);

        let window = adw::ApplicationWindow::builder()
            .application(
                &gio::Application::default()
                    .and_downcast::<adw::Application>()
                    .expect("the application is running"),
            )
            .title("Roséline Setup")
            .default_width(576)
            .default_height(760)
            .content(&nav)
            .build();

        let setup = Rc::new(Setup {
            window,
            launch_check,
            completed,
            finishing: Cell::new(false),
            uri,
        });

        let weak = Rc::downgrade(&setup);
        done.connect_clicked(move |_| {
            if let Some(setup) = weak.upgrade() {
                setup.finish();
            }
        });

        let weak = Rc::downgrade(&setup);
        setup.window.connect_close_request(move |_| {
            let Some(setup) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if !setup.completed.get() {
                SETUP.with(|slot| slot.borrow_mut().take());
                return glib::Propagation::Proceed;
            }
            setup.finish();
            glib::Propagation::Stop
        });

        setup
    }

    /// write the marker, close, and continue into a launch when asked.
    fn finish(self: &Rc<Self>) {
        if !self.completed.get() || self.finishing.replace(true) {
            return;
        }
        let paths = Paths::get();
        let _ = std::fs::create_dir_all(&paths.data);
        let _ = std::fs::write(paths.data.join(MARKER), b"");
        let launch_now = self.launch_check.is_active();
        self.window.destroy();
        SETUP.with(|slot| slot.borrow_mut().take());
        if launch_now {
            super::splash::start(self.uri.clone());
        }
    }
}

fn root_page(
    nav: &adw::NavigationView,
    settings: Rc<RefCell<Settings>>,
    selected: Rc<RefCell<Option<String>>>,
    fetch: adw::NavigationPage,
    start_fetch: Rc<dyn Fn()>,
) -> adw::NavigationPage {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(28)
        .margin_bottom(28)
        .margin_start(24)
        .margin_end(24)
        .build();

    let heading = gtk::Label::builder()
        .label("Set up Roséline")
        .css_classes(["title-1"])
        .halign(gtk::Align::Start)
        .build();
    content.append(&heading);

    let intro = gtk::Label::builder()
        .label(
            "Choose how Roséline should run Roblox. Everything here can be changed later in Settings.",
        )
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    content.append(&intro);

    let graphics = adw::PreferencesGroup::builder().title("Graphics").build();
    graphics.add(&super::graphics_page::device_row(&settings));
    graphics.add(&super::graphics_page::api_row(&settings));
    content.append(&graphics);
    content.append(&server_location_group(&settings));
    content.append(&discord_group(&settings));
    content.append(&player_group(&settings));
    content.append(&advanced_section(&settings, &selected));

    let note = gtk::Label::builder()
        .use_markup(true)
        .label("<b>More can be configured in Roséline's dedicated Settings window.</b>")
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim-label", "caption"])
        .build();
    content.append(&note);

    let next = gtk::Button::builder()
        .label("Next")
        .halign(gtk::Align::Center)
        .width_request(180)
        .margin_top(4)
        .css_classes(["suggested-action", "pill"])
        .build();
    let fetch_for_next = fetch.clone();
    let nav_for_next = nav.clone();
    next.connect_clicked(move |_| {
        if nav_for_next.find_page("fetch").is_some() {
            nav_for_next.pop_to_tag("fetch");
        } else {
            nav_for_next.push(&fetch_for_next);
        }
        start_fetch();
    });
    content.append(&next);

    let clamp = adw::Clamp::builder()
        .maximum_size(560)
        .tightening_threshold(420)
        .child(&content)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header("Roséline Setup"));
    view.set_content(Some(&scroll));

    adw::NavigationPage::builder()
        .title("Roséline Setup")
        .tag("setup")
        .child(&view)
        .build()
}

fn server_location_group(settings: &Rc<RefCell<Settings>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Server Location")
        .build();

    let enabled = adw::SwitchRow::builder()
        .title("Show server location")
        .subtitle("Sends a notification with the server's city and country after joining.")
        .active(settings.borrow().server_location.enabled)
        .build();
    enabled.add_prefix(&row_prefix("mark-location-symbolic"));
    enabled.connect_active_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().server_location.enabled = row.is_active();
            let _ = settings.borrow().save();
        }
    ));
    group.add(&enabled);
    group
}

fn discord_group(settings: &Rc<RefCell<Settings>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Discord RPC")
        .build();

    let enabled = adw::SwitchRow::builder()
        .title("Enable")
        .subtitle(
            "Shows a Discord Rich Presence in your profile whenever you play Roblox with Roséline.",
        )
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

    let join = adw::SwitchRow::builder()
        .title("Let People Join")
        .subtitle("Adds a Join Game button that when clicked, opens a link that directs to the server you're in.")
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
        }
    ));
    group.add(&avatar);

    group
}

/// the folded Advanced block: the runner picker and the environment lists.
fn player_group(settings: &Rc<RefCell<Settings>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Player").build();

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
    group.add(&close);

    let old = adw::SwitchRow::builder()
        .title("Old avatar editor background")
        .subtitle("Swaps the avatar editor background with the classic one.")
        .active(settings.borrow().avatar_background.old)
        .build();
    old.add_prefix(&row_prefix("camera-photo-symbolic"));
    old.connect_active_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            settings.borrow_mut().avatar_background.old = row.is_active();
            let _ = settings.borrow().save();
        }
    ));
    group.add(&old);
    group
}

fn advanced_section(
    settings: &Rc<RefCell<Settings>>,
    selected: &Rc<RefCell<Option<String>>>,
) -> gtk::Widget {
    let toggle = adw::ActionRow::builder()
        .title("Advanced")
        .subtitle("Rosemary and environment variables.")
        .activatable(true)
        .build();
    toggle.add_prefix(&row_prefix("system-run-symbolic"));
    let chevron = gtk::Image::from_icon_name("pan-down-symbolic");
    chevron.add_css_class("dim-label");
    toggle.add_suffix(&chevron);

    let toggle_group = adw::PreferencesGroup::new();
    toggle_group.add(&toggle);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();
    content.append(&rosemary_group(settings, selected));
    content.append(&runner_page::environment_group(
        settings,
        "Mac program environment",
        "Passed to the Mac programs Rosemary runs, as ROSEMARY_ENVIRON. NAME=VALUE, or a bare NAME to copy this computer's value.",
        |settings| &mut settings.environment.guest,
    ));
    content.append(&runner_page::environment_group(
        settings,
        "Rosemary environment",
        "Set for Rosemary's own process: drivers, tuning, and the like. NAME=VALUE, or a bare NAME to copy this computer's value.",
        |settings| &mut settings.environment.runner,
    ));

    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(200)
        .reveal_child(false)
        .child(&content)
        .build();
    let revealer_weak = revealer.downgrade();
    let chevron_weak = chevron.downgrade();
    toggle.connect_activated(move |_| {
        let Some(revealer) = revealer_weak.upgrade() else {
            return;
        };
        let open = !revealer.reveals_child();
        revealer.set_reveal_child(open);
        if let Some(chevron) = chevron_weak.upgrade() {
            chevron.set_icon_name(Some(if open {
                "pan-up-symbolic"
            } else {
                "pan-down-symbolic"
            }));
        }
    });

    let wrapper = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();
    wrapper.append(&toggle_group);
    wrapper.append(&revealer);
    wrapper.upcast()
}

#[derive(Clone)]
struct RunnerEntry {
    tag: String,
    /// installed runner id, when this version is on disk.
    id: Option<String>,
}

/// the same installed-and-remote list the Advanced settings page shows, but a
/// pick here is installed when Next is pressed, not on selection.
fn rosemary_group(
    settings: &Rc<RefCell<Settings>>,
    selected: &Rc<RefCell<Option<String>>>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Rosemary").build();
    let combo = adw::ComboRow::builder()
        .title("Rosemary version")
        .subtitle("Choose a version of Rosemary to choose.")
        .build();
    combo.add_prefix(&row_prefix("system-run-symbolic"));

    let entries: Rc<RefCell<Vec<RunnerEntry>>> = Rc::new(RefCell::new(
        crate::runner::installed()
            .into_iter()
            .map(|(id, tag)| RunnerEntry { tag, id: Some(id) })
            .collect(),
    ));
    let loading = Rc::new(Cell::new(false));

    let rebuild: Rc<dyn Fn()> = {
        let combo = combo.clone();
        let entries = entries.clone();
        let settings = settings.clone();
        let selected = selected.clone();
        let loading = loading.clone();
        Rc::new(move || {
            loading.set(true);
            let model = gtk::StringList::new(&[]);
            let pinned = settings.borrow().runner.clone();
            let preferred = selected.borrow().clone();
            let mut preferred_index = None;
            let mut pinned_index = None;
            let mut newest_installed = None;
            for (index, entry) in entries.borrow().iter().enumerate() {
                model.append(&match &entry.id {
                    Some(_) => format!("{} (installed)", entry.tag),
                    None => entry.tag.clone(),
                });
                if entry.id.is_some() && newest_installed.is_none() {
                    newest_installed = Some(index as u32);
                }
                if preferred.as_deref() == Some(entry.tag.as_str()) {
                    preferred_index = Some(index as u32);
                }
                if entry.id.as_deref() == pinned.as_deref() && entry.id.is_some() {
                    pinned_index = Some(index as u32);
                }
            }
            let chosen = preferred_index
                .or(pinned_index)
                .or(newest_installed)
                .unwrap_or(0);
            if model.n_items() == 0 {
                model.append("No versions found yet");
            } else if let Some(entry) = entries.borrow().get(chosen as usize) {
                *selected.borrow_mut() = Some(entry.tag.clone());
            }
            combo.set_model(Some(&model));
            combo.set_selected(chosen);
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
        selected,
        move |row| {
            if loading.get() {
                return;
            }
            let Some(entry) = entries.borrow().get(row.selected() as usize).cloned() else {
                return;
            };
            *selected.borrow_mut() = Some(entry.tag.clone());
            if let Some(id) = entry.id {
                settings.borrow_mut().runner = Some(id);
                let _ = settings.borrow().save();
            }
        }
    ));
    group.add(&combo);

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
                        entries.push(RunnerEntry { tag, id: None });
                    }
                }
                entries.sort_by(|a, b| {
                    crate::runner::version_key(&b.tag).cmp(&crate::runner::version_key(&a.tag))
                });
            }
            rebuild();
        }
    ));

    group
}

fn fetch_page(
    selected: Rc<RefCell<Option<String>>>,
    completed: Rc<Cell<bool>>,
    launch_check: gtk::CheckButton,
) -> (adw::NavigationPage, Rc<dyn Fn()>, gtk::Button) {
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(220)
        .vexpand(true)
        .hexpand(true)
        .build();

    let logo = gtk::Image::from_resource(LOGO_RESOURCE);
    logo.set_pixel_size(96);
    let title = gtk::Label::builder()
        .use_markup(true)
        .label("<b>Fetching Roblox</b>")
        .css_classes(["title-1"])
        .halign(gtk::Align::Center)
        .build();
    let description = gtk::Label::builder()
        .label("Roséline is downloading Rosemary and the Roblox client. The first run can take a few minutes.")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .width_request(380)
        .css_classes(["dim-label"])
        .halign(gtk::Align::Center)
        .build();
    let bar = gtk::ProgressBar::builder()
        .width_request(340)
        .show_text(false)
        .build();
    let spinner = gtk::Spinner::new();
    spinner.set_size_request(16, 16);
    spinner.set_spinning(true);
    let stage = gtk::Label::builder()
        .label("Getting things ready…")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .width_request(380)
        .css_classes(["dim-label", "caption"])
        .build();
    let stage_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();
    stage_row.append(&spinner);
    stage_row.append(&stage);

    let fetching = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Center)
        .margin_top(36)
        .margin_bottom(36)
        .margin_start(36)
        .margin_end(36)
        .build();
    logo.set_halign(gtk::Align::Center);
    bar.set_halign(gtk::Align::Center);
    fetching.append(&logo);
    fetching.append(&title);
    fetching.append(&description);
    fetching.append(&bar);
    fetching.append(&stage_row);
    stack.add_named(&fetching, Some("fetching"));

    let check = gtk::Image::from_icon_name("object-select-symbolic");
    check.set_pixel_size(112);
    check.add_css_class("accent");
    check.set_halign(gtk::Align::Center);
    let done_title = gtk::Label::builder()
        .use_markup(true)
        .label("<b>Roséline is ready to use</b>")
        .css_classes(["title-1"])
        .halign(gtk::Align::Center)
        .build();
    let done = gtk::Button::builder()
        .label("Done")
        .halign(gtk::Align::Center)
        .width_request(180)
        .margin_top(6)
        .css_classes(["suggested-action", "pill"])
        .build();
    let done_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .valign(gtk::Align::Center)
        .margin_top(36)
        .margin_bottom(36)
        .margin_start(36)
        .margin_end(36)
        .build();
    done_box.append(&check);
    done_box.append(&done_title);
    done_box.append(&launch_check);
    done_box.append(&done);
    stack.add_named(&done_box, Some("done"));

    let warning = gtk::Image::from_icon_name("dialog-warning-symbolic");
    warning.set_pixel_size(96);
    warning.set_halign(gtk::Align::Center);
    let error_title = gtk::Label::builder()
        .use_markup(true)
        .label("<b>Couldn't fetch Roblox</b>")
        .css_classes(["title-1"])
        .halign(gtk::Align::Center)
        .build();
    let error_body = gtk::Label::builder()
        .label("")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .width_request(400)
        .selectable(true)
        .css_classes(["dim-label"])
        .halign(gtk::Align::Center)
        .build();
    let retry = gtk::Button::builder()
        .label("Try again")
        .halign(gtk::Align::Center)
        .width_request(180)
        .margin_top(6)
        .css_classes(["suggested-action", "pill"])
        .build();
    let error_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .valign(gtk::Align::Center)
        .margin_top(36)
        .margin_bottom(36)
        .margin_start(36)
        .margin_end(36)
        .build();
    error_box.append(&warning);
    error_box.append(&error_title);
    error_box.append(&error_body);
    error_box.append(&retry);
    stack.add_named(&error_box, Some("error"));

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header("Fetching Roblox"));
    view.set_content(Some(&stack));

    let page = adw::NavigationPage::builder()
        .title("Fetching Roblox")
        .tag("fetch")
        .child(&view)
        .build();

    let start: Rc<dyn Fn()> = {
        let page = page.clone();
        let stack = stack.clone();
        let bar = bar.clone();
        let stage = stage.clone();
        let spinner = spinner.clone();
        let error_body = error_body.clone();
        let completed = completed.clone();
        let selected = selected.clone();
        Rc::new(move || {
            page.set_can_pop(false);
            stack.set_visible_child_name("fetching");
            bar.set_fraction(0.0);
            stage.set_label("Getting things ready…");
            spinner.set_spinning(true);

            let (sender, receiver) = async_channel::bounded(32);
            let progress = Progress::new(sender);
            let tag = selected.borrow().clone();
            std::thread::spawn(move || match launch::prepare(&progress, tag.as_deref()) {
                Ok(()) => progress.done(),
                Err(error) => progress.failed(format!("{error:#}")),
            });

            let state = Rc::new(RefCell::new(FetchState::default()));
            glib::spawn_future_local(glib::clone!(
                #[strong]
                page,
                #[strong]
                stack,
                #[strong]
                bar,
                #[strong]
                stage,
                #[strong]
                spinner,
                #[strong]
                error_body,
                #[strong]
                completed,
                async move {
                    while let Ok(event) = receiver.recv().await {
                        match event {
                            Event::Stage(text) => {
                                let fraction = state.borrow_mut().stage(&text);
                                if let Some(fraction) = fraction {
                                    bar.set_fraction(fraction);
                                }
                                stage.set_label(&text);
                            }
                            Event::Progress(fraction) => {
                                bar.set_fraction(state.borrow().download(fraction));
                            }
                            Event::Done => {
                                completed.set(true);
                                spinner.set_spinning(false);
                                bar.set_fraction(1.0);
                                stack.set_visible_child_name("done");
                                break;
                            }
                            Event::Failed(text) => {
                                spinner.set_spinning(false);
                                page.set_can_pop(true);
                                error_body.set_label(&short_error(&text));
                                stack.set_visible_child_name("error");
                                break;
                            }
                            Event::Spawned(_) => {}
                        }
                    }
                }
            ));
        })
    };

    let retry_start = start.clone();
    retry.connect_clicked(move |_| retry_start());

    (page, start, done)
}

fn header(title: &str) -> adw::HeaderBar {
    let menu = gio::Menu::new();
    menu.append(Some("About Roséline"), Some("app.about"));
    menu.append(Some("Quit"), Some("app.quit"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Menu")
        .build();

    let bar = adw::HeaderBar::builder()
        .title_widget(&adw::WindowTitle::new(title, ""))
        .build();
    bar.pack_end(&menu_button);
    bar
}

fn short_error(text: &str) -> String {
    let mut summary: String = text.chars().take(400).collect();
    if text.chars().count() > 400 {
        summary.push('…');
    }
    summary
}

#[derive(Default)]
struct FetchState {
    segment: Segment,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Segment {
    #[default]
    Idle,
    Rosemary,
    Prefix,
    Client,
    Unpack,
}

impl FetchState {
    /// map a pipeline stage line onto the bar.
    fn stage(&mut self, text: &str) -> Option<f64> {
        if text.contains("Verifying") {
            self.segment = Segment::Rosemary;
            Some(0.14)
        } else if text.contains("Rosemary") {
            self.segment = Segment::Rosemary;
            Some(0.03)
        } else if text.contains("prefix") {
            self.segment = Segment::Prefix;
            Some(0.18)
        } else if text.contains("Downloading the Roblox client") {
            self.segment = Segment::Client;
            Some(0.30)
        } else if text.contains("Unpacking the Roblox client") {
            self.segment = Segment::Unpack;
            Some(unpack_fraction(text).map_or(0.78, |fraction| 0.78 + 0.16 * fraction))
        } else if text.contains("Applying the content overlay") {
            self.segment = Segment::Idle;
            Some(0.96)
        } else {
            None
        }
    }

    /// download bytes fill the part each stage owns.
    fn download(&self, fraction: f64) -> f64 {
        match self.segment {
            Segment::Rosemary => 0.03 + 0.10 * fraction,
            Segment::Client => 0.30 + 0.46 * fraction,
            Segment::Unpack => 0.78,
            Segment::Prefix | Segment::Idle => 0.18,
        }
        .clamp(0.0, 1.0)
    }
}

fn unpack_fraction(text: &str) -> Option<f64> {
    let inside = text.split_once('(')?.1.split_once(')')?.0;
    let (done, total) = inside.split_once('/')?;
    let done: f64 = done.trim().parse().ok()?;
    let total: f64 = total.trim().parse().ok()?;
    (total > 0.0).then(|| (done / total).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_walk_the_bar() {
        let mut state = FetchState::default();
        assert_eq!(
            state.stage("Checking for the latest Rosemary release"),
            Some(0.03)
        );
        assert!((state.download(0.5) - 0.08).abs() < 1e-9);
        assert_eq!(state.stage("Verifying the download"), Some(0.14));
        assert_eq!(
            state.stage("Creating the prefix and downloading the Darwin base"),
            Some(0.18)
        );
        assert_eq!(
            state.stage("Downloading the Roblox client 0.700"),
            Some(0.30)
        );
        assert!((state.download(0.5) - 0.53).abs() < 1e-9);
        let unpacked = state.stage("Unpacking the Roblox client (50/100)").unwrap();
        assert!((unpacked - 0.86).abs() < 1e-9);
        assert_eq!(state.stage("Applying the content overlay"), Some(0.96));
    }

    #[test]
    fn unpack_counts_parse() {
        assert_eq!(
            unpack_fraction("Unpacking the Roblox client (128/1024)"),
            Some(0.125)
        );
        assert_eq!(unpack_fraction("Unpacking the Roblox client"), None);
        assert_eq!(unpack_fraction("Unpacking the Roblox client (1/0)"), None);
    }
}
