// SPDX-License-Identifier: GPL-3.0-or-later

//! the splash: Roblox's icon and a progress bar fed by Rosemary and the
//! guest's logs, in the shape of Roblox's own bootstrapper.

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Child;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::glib;

use crate::launch;
use crate::progress::{Event, Progress, Spawned};
use crate::rpc::Discord;
use crate::session::{self, Session};
use crate::settings::Settings;

const LOGO_RESOURCE: &str = "/net/buwryy/Roseline/icons/scalable/apps/net.buwryy.Roseline.svg";
/// how long a jump of the bar takes, in seconds.
const ANIMATION: f64 = 0.111;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Phase {
    #[default]
    Idle,
    Working,
    Running,
}

#[derive(Clone)]
struct Game {
    name: String,
    cover_url: String,
    join_url: String,
    page_url: String,
    private: bool,
    started: u64,
}

#[derive(Default)]
struct Inner {
    phase: Phase,
    hold: Option<gtk::gio::ApplicationHoldGuard>,
    child: Option<Child>,
    rpc: Option<Discord>,
    pid: Option<u32>,
    session: Option<Session>,
    timer: Option<glib::SourceId>,
    target: u8,
    shown: f64,
    animation: Option<(Instant, f64)>,
    animating: bool,
    game: Option<Game>,
    started: Option<Instant>,
    rpc_attempts: u32,
}

thread_local! {
    static SPLASH: RefCell<Option<Rc<Splash>>> = const { RefCell::new(None) };
}

pub struct Splash {
    window: adw::ApplicationWindow,
    bar: gtk::ProgressBar,
    inner: RefCell<Inner>,
}

/// re-read the Discord settings and update the live presence.
pub fn refresh_presence() {
    if let Some(splash) = SPLASH.with(|slot| slot.borrow().clone()) {
        splash.refresh_presence();
    }
}

/// the way in: a desktop click, or a link.
pub fn start(uri: Option<String>) {
    if launch::running_pid().is_some() {
        already_running_dialog();
        return;
    }
    if !super::setup::is_complete() {
        super::setup::start(uri);
        return;
    }
    if let Some(existing) = SPLASH.with(|slot| slot.borrow().clone())
        && existing.is_active()
    {
        existing.window.present();
        return;
    }

    let splash = Rc::new(Splash::new());
    SPLASH.with(|slot| *slot.borrow_mut() = Some(splash.clone()));

    let splash_weak = Rc::downgrade(&splash);
    splash.window.connect_close_request(move |window| {
        // closing hides the window; the work itself is not cancelled.
        if splash_weak
            .upgrade()
            .is_some_and(|splash| splash.is_active())
        {
            window.set_visible(false);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });

    splash.window.present();
    splash.begin(uri);
}

impl Splash {
    fn new() -> Splash {
        let icon = match roblox_icon() {
            Some(texture) => gtk::Image::from_paintable(Some(&texture)),
            None => gtk::Image::from_resource(LOGO_RESOURCE),
        };
        icon.set_pixel_size(96);

        let title = gtk::Label::builder()
            .use_markup(true)
            .label("<b>Launching Roblox...</b>")
            .css_classes(["title-1"])
            .build();

        let bar = gtk::ProgressBar::builder()
            .width_request(420)
            .show_text(false)
            .build();
        bar.set_margin_top(12);

        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_top(44)
            .spacing(18)
            .vexpand(true)
            .build();
        column.append(&icon);
        column.append(&title);
        column.append(&bar);

        let window = adw::ApplicationWindow::builder()
            .application(
                &gtk::gio::Application::default()
                    .and_downcast::<adw::Application>()
                    .expect("the application is running"),
            )
            .title(crate::config::DISPLAY_NAME)
            .default_width(460)
            .default_height(270)
            .resizable(false)
            .content(&column)
            .build();

        Splash {
            window,
            bar,
            inner: RefCell::new(Inner::default()),
        }
    }

    pub fn is_active(&self) -> bool {
        self.inner.borrow().phase != Phase::Idle
    }

    /// set a new target and ease towards it; a jump takes 111 ms.
    fn set_percent(self: &Rc<Self>, percent: u8) {
        let mut inner = self.inner.borrow_mut();
        inner.target = if inner.phase == Phase::Working {
            inner.target.max(percent.min(100))
        } else {
            percent.min(100)
        };
        inner.animation = Some((Instant::now(), inner.shown));
        drop(inner);
        self.tick_animation();
    }

    fn tick_animation(self: &Rc<Self>) {
        if self.inner.borrow().animating {
            return;
        }
        self.inner.borrow_mut().animating = true;

        let splash = self.clone();
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let (fraction, done) = {
                let mut inner = splash.inner.borrow_mut();
                let target = f64::from(inner.target);
                let shown = match inner.animation {
                    Some((start, from)) => {
                        let t = (start.elapsed().as_secs_f64() / ANIMATION).clamp(0.0, 1.0);
                        let eased = 1.0 - (1.0 - t).powi(3);
                        from + (target - from) * eased
                    }
                    None => target,
                };
                let done = inner.animation.is_none() || (shown - target).abs() < 0.01;
                if done {
                    inner.shown = target;
                    inner.animation = None;
                    inner.animating = false;
                } else {
                    inner.shown = shown;
                }
                (inner.shown / 100.0, done)
            };
            splash.bar.set_fraction(fraction);
            if done {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    fn begin(self: &Rc<Self>, uri: Option<String>) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.phase = Phase::Working;
            inner.target = 0;
            inner.game = None;
            inner.hold = gtk::gio::Application::default().map(|app| app.hold());
        }
        self.set_percent(2);
        self.check_sandbox_rpc();

        let (sender, receiver) = async_channel::bounded(16);
        let progress = Progress::new(sender);
        let splash = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = receiver.recv().await {
                splash.on_event(event);
            }
        });

        std::thread::spawn(move || match launch::run(&progress, uri.as_deref()) {
            Ok(child) => {
                let settings = Settings::load();
                let pid = child.id();
                let rpc = if settings.discord.enabled {
                    let app_id = settings.discord.app_id.trim();
                    (!app_id.is_empty())
                        .then(|| crate::rpc::start(app_id, pid, &launch::idle_activity(&settings)))
                        .flatten()
                } else {
                    None
                };
                progress.spawned(Spawned { child, rpc, pid });
            }
            Err(error) => progress.failed(format!("{error:#}")),
        });
    }

    fn on_event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Stage(text) => {
                if self.inner.borrow().phase == Phase::Working
                    && let Some(percent) = stage_percent(&text)
                {
                    self.set_percent(percent);
                }
            }
            Event::Progress(fraction) => {
                if self.inner.borrow().phase == Phase::Working {
                    self.set_percent(8 + (fraction * 42.0) as u8);
                }
            }
            Event::Failed(text) => self.failed(&text),
            Event::Spawned(spawned) => self.adopt(spawned),
            Event::Done => {}
        }
    }

    fn failed(self: &Rc<Self>, text: &str) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.phase = Phase::Idle;
            inner.hold = None;
        }
        eprintln!("roseline: launch failed: {text}");
        let summary: String = text.chars().take(240).collect();
        let dialog = adw::AlertDialog::builder()
            .heading("Couldn't start Roblox")
            .body(summary)
            .build();
        dialog.add_response("ok", "Got it");
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("ok");
        dialog.present(None::<&gtk::Window>);
        self.window.destroy();
        SPLASH.with(|slot| *slot.borrow_mut() = None);
    }

    fn adopt(self: &Rc<Self>, spawned: Spawned) {
        let Spawned { child, rpc, pid } = spawned;
        {
            let mut inner = self.inner.borrow_mut();
            inner.phase = Phase::Running;
            inner.child = Some(child);
            inner.rpc = rpc;
            inner.pid = Some(pid);
            inner.session = Some(Session::new());
            inner.started = Some(Instant::now());
        }
        self.set_percent(60);

        let splash = self.clone();
        let timer = glib::timeout_add_local(Duration::from_millis(250), move || splash.tick());
        self.inner.borrow_mut().timer = Some(timer);

        // if the guest never says anything useful, do not sit here forever.
        let splash = self.clone();
        glib::timeout_add_seconds_local_once(40, move || {
            if splash.inner.borrow().phase == Phase::Running && splash.window.is_visible() {
                splash.complete();
            }
        });
    }

    fn tick(self: &Rc<Self>) -> glib::ControlFlow {
        let exited = {
            let mut inner = self.inner.borrow_mut();
            match inner.child.as_mut() {
                Some(child) => matches!(child.try_wait(), Ok(Some(_)) | Err(_)),
                None => true,
            }
        };
        if exited {
            self.finish();
            return glib::ControlFlow::Break;
        }
        self.ensure_rpc();
        let events = {
            let mut inner = self.inner.borrow_mut();
            match inner.session.as_mut() {
                Some(session) => session.tick(),
                None => Vec::new(),
            }
        };
        for event in events {
            match event {
                session::Event::Progress(percent, _caption) => {
                    self.set_percent(percent);
                    // the app paints its window right around this marker;
                    // earlier milestones run seconds ahead of it.
                    if percent >= 92 {
                        self.complete();
                    }
                }
                session::Event::Joining(joining) => {
                    self.set_percent(90);
                    self.resolve_game(joining);
                }
                session::Event::Left => self.left_game(),
            }
        }
        glib::ControlFlow::Continue
    }

    /// bow out once the game is really up.
    fn complete(self: &Rc<Self>) {
        self.window.set_visible(false);
    }

    /// A Flatpak sandbox cannot see the Discord bridge's socket until the
    /// user grants it; say so once, and only while the presence is wanted.
    fn check_sandbox_rpc(&self) {
        if !crate::rpc::sandboxed() || !Settings::load().discord.enabled {
            return;
        }
        let marker = crate::config::Paths::get().data.join("rpc-hint-shown");
        if marker.exists() || crate::rpc::socket_available() {
            return;
        }
        let _ = std::fs::create_dir_all(crate::config::Paths::get().data);
        let _ = std::fs::write(&marker, b"");
        self.sandbox_rpc_dialog();
    }

    /// The one-time override, copyable, with the restart spelled out.
    fn sandbox_rpc_dialog(&self) {
        let command = format!(
            "flatpak override --user {} --filesystem=xdg-run",
            crate::config::APP_ID
        );
        let line = gtk::Label::builder()
            .label(&command)
            .selectable(true)
            .wrap(true)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["monospace"])
            .build();

        let copy = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copy the command")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        copy.connect_clicked(glib::clone!(
            #[weak]
            line,
            move |button| {
                button.display().clipboard().set_text(&line.text());
                button.set_icon_name("object-select-symbolic");
                button.set_tooltip_text(Some("Copied"));
            }
        ));

        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();
        row.append(&line);
        row.append(&copy);

        let dialog = adw::AlertDialog::builder()
            .heading("Allow Discord Rich Presence")
            .body("Roseline is sandboxed and cannot reach the Discord bridge's socket. Run this command, then restart Roseline.")
            .extra_child(&row)
            .build();
        dialog.add_response("ok", "Got it");
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("ok");
        dialog.present(Some(&self.window));
    }

    /// retry the bridge every fifth tick; never downgrade an in-game
    /// presence.
    fn ensure_rpc(self: &Rc<Self>) {
        {
            let inner = self.inner.borrow();
            if inner.rpc.is_some() || inner.game.is_some() || inner.pid.is_none() {
                return;
            }
        }
        let settings = Settings::load();
        let app_id = settings.discord.app_id.trim().to_string();
        if !settings.discord.enabled || app_id.is_empty() {
            return;
        }
        {
            let mut inner = self.inner.borrow_mut();
            inner.rpc_attempts = inner.rpc_attempts.wrapping_add(1);
            if inner.rpc_attempts % 5 != 0 {
                return;
            }
        }
        let Some(pid) = self.inner.borrow().pid else {
            return;
        };
        if let Some(rpc) = crate::rpc::start(&app_id, pid, &launch::idle_activity(&settings)) {
            self.inner.borrow_mut().rpc = Some(rpc);
        }
    }

    fn resolve_game(self: &Rc<Self>, joining: session::Joining) {
        let join_url = joining.join_url();
        let page_url = joining.page_url();
        let private = joining.private;
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let resolved = crate::gameinfo::resolve(joining.place_id).ok().map(|info| {
                let cover = if info.cover.is_empty() {
                    None
                } else {
                    crate::net::get_bytes(&info.cover).ok()
                };
                (info.name, info.cover, cover)
            });
            let _ = sender.send_blocking(resolved);
        });
        let splash = self.clone();
        glib::spawn_future_local(async move {
            let Ok(Some((name, cover_url, _cover))) = receiver.recv().await else {
                return;
            };
            splash.show_game(name, cover_url, join_url, page_url, private);
        });
    }

    fn show_game(
        self: &Rc<Self>,
        name: String,
        cover_url: String,
        join_url: String,
        page_url: String,
        private: bool,
    ) {
        if self.inner.borrow().phase != Phase::Running {
            return;
        }
        self.set_percent(100);
        self.complete();

        let game = Game {
            name,
            cover_url,
            join_url,
            page_url,
            private,
            started: launch::now(),
        };
        self.inner.borrow_mut().game = Some(game.clone());

        let settings = Settings::load();
        self.apply_presence(Some(&game), &settings);
    }

    /// the settings window changed a Discord toggle while a game is running.
    fn refresh_presence(self: &Rc<Self>) {
        let (phase, game) = {
            let inner = self.inner.borrow();
            (inner.phase, inner.game.clone())
        };
        if phase != Phase::Running {
            return;
        }
        let settings = Settings::load();
        self.apply_presence(game.as_ref(), &settings);
    }

    fn apply_presence(self: &Rc<Self>, game: Option<&Game>, settings: &Settings) {
        let activity = if settings.discord.show_game {
            if let Some(game) = game {
                crate::rpc::Activity {
                    details: game.name.clone(),
                    state: settings.discord.state.clone(),
                    started: game.started,
                    large_image: (!game.cover_url.is_empty())
                        .then(|| game.cover_url.clone())
                        .or_else(|| Some(crate::rpc::ROBLOX_LOGO.to_string())),
                    small_image: Some(crate::rpc::ROBLOX_LOGO.to_string()),
                    button: Some(game_button(settings.discord.join_button, game)),
                }
            } else {
                launch::idle_activity(settings)
            }
        } else {
            launch::idle_activity(settings)
        };
        let mut inner = self.inner.borrow_mut();
        let pid = inner.pid;
        if let (Some(rpc), Some(pid)) = (inner.rpc.as_mut(), pid) {
            rpc.set(pid, &activity);
        }
    }

    fn left_game(self: &Rc<Self>) {
        let settings = Settings::load();
        let mut inner = self.inner.borrow_mut();
        inner.game = None;
        let pid = inner.pid;
        if let (Some(rpc), Some(pid)) = (inner.rpc.as_mut(), pid) {
            rpc.set(pid, &launch::idle_activity(&settings));
        }
    }

    fn finish(self: &Rc<Self>) {
        let (pid, closed, early) = {
            let mut inner = self.inner.borrow_mut();
            if let Some(timer) = inner.timer.take() {
                timer.remove();
            }
            let pid = inner.pid;
            if let (Some(rpc), Some(pid)) = (inner.rpc.as_mut(), pid) {
                rpc.clear(pid);
            }
            let closed = inner.game.take();
            let early = inner
                .started
                .take()
                .is_some_and(|started| started.elapsed().as_secs() < 15);
            inner.rpc = None;
            inner.child = None;
            inner.pid = None;
            inner.session = None;
            inner.hold = None;
            inner.phase = Phase::Idle;
            inner.target = 0;
            (pid, closed, early)
        };
        if pid.is_some() {
            let _ = std::fs::remove_file(crate::config::Paths::get().game_pid());
        }
        self.window.destroy();
        SPLASH.with(|slot| *slot.borrow_mut() = None);
        if pid.is_some() && closed.is_none() && early {
            early_exit_dialog();
        }
    }
}

fn game_button(join_enabled: bool, game: &Game) -> crate::rpc::Button {
    if join_enabled && !game.private {
        crate::rpc::Button {
            label: "Join Game",
            url: game.join_url.clone(),
        }
    } else {
        crate::rpc::Button {
            label: "View Game",
            url: game.page_url.clone(),
        }
    }
}

/// Roblox's icon from the managed copy; no icns loader is guaranteed on a
/// Linux host.
fn roblox_icon() -> Option<gtk::gdk::Texture> {
    let resources = crate::config::Paths::get()
        .app_bundle()
        .join("Contents/Resources");
    let mut fallback: Option<PathBuf> = None;
    let mut chosen: Option<PathBuf> = None;
    for entry in std::fs::read_dir(resources).ok()?.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("icns"))
        {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.contains("appicon") || name.contains("roblox") {
                chosen = Some(path);
                break;
            }
            fallback.get_or_insert(path);
        }
    }
    let bytes = std::fs::read(chosen.or(fallback)?).ok()?;
    let png = png_inside(&bytes)?;
    gtk::gdk::Texture::from_bytes(&glib::Bytes::from(png)).ok()
}

/// the largest PNG in an `.icns`, or the bytes when they already are one.
fn png_inside(bytes: &[u8]) -> Option<&[u8]> {
    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.starts_with(&PNG) {
        return Some(bytes);
    }
    if bytes.len() < 8 || &bytes[..4] != b"icns" {
        return None;
    }
    let mut best: Option<&[u8]> = None;
    let mut offset = 8;
    while offset + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        if length < 8 || offset + length > bytes.len() {
            break;
        }
        let data = &bytes[offset + 8..offset + length];
        if data.starts_with(&PNG) && best.is_none_or(|previous| previous.len() < data.len()) {
            best = Some(data);
        }
        offset += length;
    }
    best
}

/// the one-instance dialog.
fn already_running_dialog() {
    let dialog = adw::AlertDialog::builder()
        .heading("Roseline is already running")
        .build();
    dialog.add_response("ok", "Got it");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    dialog.present(None::<&gtk::Window>);
}

/// the guest fell over right after starting; say so and offer the logs.
fn early_exit_dialog() {
    let dialog = adw::AlertDialog::builder()
        .heading("Roblox crashed!")
        .body_use_markup(true)
        .body("Something went wrong while opening Roblox. You can <a href=\"https://github.com/buwryme/Roseline/issues\">open an issue</a>.")
        .build();
    dialog.add_response("logs", "View Logs");
    dialog.set_response_appearance("logs", adw::ResponseAppearance::Suggested);
    dialog.add_response("ok", "OK");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    dialog.connect_response(None, |_, response| {
        if response != "logs" {
            return;
        }
        let target = match newest_launch_log() {
            Some(path) => path,
            None => crate::config::Paths::get().logs(),
        };
        let launcher = gtk::FileLauncher::new(Some(&gtk::gio::File::for_path(target)));
        launcher.launch(None::<&gtk::Window>, gtk::gio::Cancellable::NONE, |_| {});
    });
    // a lone dialog would not keep the application alive.
    if let Some(app) = gtk::gio::Application::default() {
        unsafe {
            dialog.set_data("roseline-hold", app.hold());
        }
    }
    dialog.present(None::<&gtk::Window>);
}

/// the newest `launch-*.log`, the guest's own stderr.
fn newest_launch_log() -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(crate::config::Paths::get().logs())
        .ok()?
        .flatten()
    {
        if !entry.file_name().to_string_lossy().starts_with("launch-") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(time, _)| *time < modified) {
            newest = Some((modified, entry.path()));
        }
    }
    newest.map(|(_, path)| path)
}

/// coarse percentages for the pipeline's stage lines; downloads arrive as
/// fractions instead.
fn stage_percent(text: &str) -> Option<u8> {
    let markers: [(&str, u8); 8] = [
        ("Rosemary release", 3),
        ("Downloading", 5),
        ("Verifying", 7),
        ("prefix", 9),
        ("Darwin", 9),
        ("Unpacking", 52),
        ("overlay", 58),
        ("Starting Roblox", 60),
    ];
    markers
        .iter()
        .find_map(|(marker, percent)| text.contains(marker).then_some(*percent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pngs_come_out_of_icns_chunks() {
        let mut icns = Vec::new();
        icns.extend_from_slice(b"icns");
        icns.extend_from_slice(&0u32.to_be_bytes()); // patched below
        for (kind, body) in [
            (b"ic04".to_vec(), vec![0u8; 16]), // legacy, not a PNG
            (
                b"ic07".to_vec(),
                [b"\x89PNG\r\n\x1a\n".as_slice(), &[1u8; 4][..]].concat(),
            ),
            (
                b"ic13".to_vec(),
                [b"\x89PNG\r\n\x1a\n".as_slice(), &[2u8; 16][..]].concat(),
            ),
        ] {
            icns.extend_from_slice(&kind);
            icns.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
            icns.extend_from_slice(&body);
        }
        let declared = icns.len() as u32;
        icns[4..8].copy_from_slice(&declared.to_be_bytes());
        let png = png_inside(&icns).expect("a png");
        assert_eq!(png.len(), 24); // the larger of the two
    }

    #[test]
    fn plain_pngs_pass_through() {
        let bytes = b"\x89PNG\r\n\x1a\nwhatever";
        assert_eq!(png_inside(bytes).unwrap(), bytes);
        assert!(png_inside(b"not an icon").is_none());
    }

    #[test]
    fn join_button_falls_back_to_view_game() {
        let game = Game {
            name: "Blox Fruits".into(),
            cover_url: String::new(),
            join_url: "roblox://experiences/start?placeId=1&gameInstanceId=abc".into(),
            page_url: "https://www.roblox.com/games/1".into(),
            private: false,
            started: 0,
        };
        let join = game_button(true, &game);
        assert_eq!(join.label, "Join Game");
        assert!(join.url.starts_with("roblox://"));

        let view = game_button(false, &game);
        assert_eq!(view.label, "View Game");
        assert_eq!(view.url, "https://www.roblox.com/games/1");

        let private = Game {
            private: true,
            ..game
        };
        let fallback = game_button(true, &private);
        assert_eq!(fallback.label, "View Game");
        assert_eq!(fallback.url, "https://www.roblox.com/games/1");
    }
}
