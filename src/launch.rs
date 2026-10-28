// SPDX-License-Identifier: GPL-3.0-or-later

//! the launch pipeline: everything between a click and a running client.
//!
//! the environment is pinned in `docs/architecture.md`; protocol arguments
//! mirror Butter's `shortcuts.rs` (GPL-3.0-or-later, the Microlift
//! contributors).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::config::Paths;
use crate::gpu;
use crate::progress::Progress;
use crate::settings::{Backend, Settings};

use gtk::glib;

/// the pid of a running guest: the pid file plus a live Rosemary process.
pub fn running_pid() -> Option<u32> {
    let text = std::fs::read_to_string(Paths::get().game_pid()).ok()?;
    let pid: u32 = text.trim().parse().ok()?;
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let cmdline = String::from_utf8_lossy(&cmdline);
    cmdline.contains("rosemary").then_some(pid)
}

/// bring everything to order, then start the client and return the guest.
pub fn run(
    progress: &Progress,
    uri: Option<&str>,
) -> Result<(Child, async_channel::Receiver<crate::session::Event>)> {
    let paths = Paths::get();
    let mut settings = Settings::load();

    let runner_id = crate::runner::ensure(progress)?;
    // always follow the release that ensure() just checked: a downloaded
    // runner must be the one the prefix actually uses.
    if set_runner(&mut settings, &runner_id) {
        settings.save()?;
    }
    // a missing muvm, or a VM someone else holds, stops the launch here
    // rather than after the client download.
    if let Some((_, binary)) = paths.runner_selected(settings.runner.as_deref()) {
        crate::muvm::route(
            crate::rosemary::runner_dir(&binary)?,
            settings.compatibility.muvm,
        )?;
    }
    prepare_bottle(progress, &settings)?;

    progress.stage("Starting Roblox");
    let (_, binary) = paths
        .runner_selected(settings.runner.as_deref())
        .context("the Rosemary runner disappeared mid-launch")?;
    let runner_dir = crate::rosemary::runner_dir(&binary)?;
    crate::runner::refresh_launcher(runner_dir)?;
    let through_vm = crate::muvm::route(runner_dir, settings.compatibility.muvm)?;
    std::fs::create_dir_all(paths.logs())?;
    let started = now();
    let log_path = paths.logs().join(format!("launch-{started}.log"));
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let error_log = log.try_clone()?;

    let mut arguments: Vec<OsString> = vec!["run".into(), paths.app_bundle().into()];
    arguments.extend(
        guest_arguments(uri, &paths.prefix())
            .into_iter()
            .map(OsString::from),
    );
    let mut command = if through_vm {
        // the VM writes the log itself; muvm passes no output back.
        crate::muvm::command(
            &binary,
            &arguments,
            &environment(&settings),
            &paths.app(),
            &log_path,
            settings.compatibility.vm_memory,
        )?
    } else {
        let mut command = Command::new(&binary);
        command
            .args(&arguments)
            .envs(environment(&settings))
            .current_dir(paths.app());
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(error_log));
    // its own group, so a signal for Roseline never hits the game.
    std::os::unix::process::CommandExt::process_group(&mut command, 0);

    let child = command
        .spawn()
        .with_context(|| "could not start Rosemary")?;
    crate::util::write_atomic(&paths.game_pid(), child.id().to_string().as_bytes())?;
    if settings.server_location.enabled {
        spawn_toast_helper(&log_path, child.id());
    }

    // the child writes its guest log to this launch's file; tail that exact
    // file, so a stale log from another instance can never be mistaken for it.
    let (event_sender, event_receiver) = async_channel::bounded(64);
    std::thread::spawn(move || {
        let mut session = crate::session::Session::new(log_path);
        loop {
            if event_sender.is_closed() {
                return;
            }
            for event in session.tick() {
                if event_sender.send_blocking(event).is_err() {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });

    Ok((child, event_receiver))
}

fn spawn_toast_helper(log: &Path, game_pid: u32) {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    else {
        return;
    };
    let Ok(error) = file.try_clone() else {
        return;
    };
    let mut command = Command::new(executable);
    command
        .arg("--toast-helper")
        .arg(log)
        .arg(game_pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(error));
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    if let Err(error) = command.spawn() {
        eprintln!("roseline-toast: could not start the helper: {error}");
    }
}

/// what setup downloads before the first launch: a runner, the prefix, the
/// client and the overlay, with no game process.
pub fn prepare(progress: &Progress, tag: Option<&str>) -> Result<()> {
    let mut settings = Settings::load();
    let runner_id = match tag {
        Some(tag) => crate::runner::ensure_tag(progress, tag)?,
        None => crate::runner::ensure(progress)?,
    };
    if set_runner(&mut settings, &runner_id) {
        settings.save()?;
    }
    prepare_bottle(progress, &settings)
}

/// pin the config to the runner that was just checked or installed.
fn set_runner(settings: &mut Settings, runner_id: &str) -> bool {
    if settings.runner.as_deref() == Some(runner_id) {
        return false;
    }
    settings.runner = Some(runner_id.to_string());
    true
}

fn prepare_bottle(progress: &Progress, settings: &Settings) -> Result<()> {
    ensure_prefix(progress, settings)?;
    crate::roblox::ensure(progress)?;
    crate::cookies::prepare();
    crate::roblox::brand()?;
    crate::overlay::apply(progress)?;
    crate::fonts::apply(settings)?;
    crate::flags::write(settings)?;
    Ok(())
}

/// wait for the guest's process group to drain, so nothing can write the
/// cookie jar after it has been sealed.
pub fn wait_for_group(pid: u32, timeout: std::time::Duration) {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        // SAFETY: signal 0 only probes the process group.
        let alive = unsafe { libc::kill(-(pid as i32), 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        if !alive {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// take a wedged guest out without asking.
pub fn force_kill(pid: u32) {
    // SAFETY: a plain signal to a pid this process spawned.
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

/// the presence while nothing is joined: Roblox logo, with the avatar head
/// as the badge when wanted.
pub fn idle_activity(settings: &Settings) -> crate::rpc::Activity {
    crate::rpc::Activity {
        details: settings.discord.details.clone(),
        state: settings.discord.state.clone(),
        started: now(),
        large_image: Some(crate::rpc::ROBLOX_LOGO.to_string()),
        small_image: if settings.discord.show_avatar {
            crate::avatar::cached()
        } else {
            None
        },
        buttons: Vec::new(),
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn ensure_prefix(progress: &Progress, settings: &Settings) -> Result<()> {
    let paths = Paths::get();
    if paths.prefix().join("root").is_dir() {
        return Ok(());
    }
    progress.stage("Creating the prefix and downloading the Darwin base");
    let log = paths.logs().join("prefix-create.log");
    let status = crate::rosemary::create(settings, &log)?;
    if !status.success() {
        bail!("the prefix could not be created; see {}", log.display());
    }
    Ok(())
}

fn resolved_backend(settings: &Settings) -> Backend {
    match settings.graphics.backend {
        Backend::Auto if gpu::vulkan_available() => Backend::Vulkan,
        Backend::Auto => Backend::OpenGL,
        backend => backend,
    }
}

/// Rosemary's environment for the current settings.
pub fn environment(settings: &Settings) -> Vec<(String, String)> {
    let mut environment = vec![
        (
            "ROSEMARY_PREFIX".to_string(),
            Paths::get().prefix().display().to_string(),
        ),
        ("ROSEMARY_DECORATIONS".to_string(), "server".to_string()),
        ("ROSEMARY_MENU".to_string(), "none".to_string()),
        ("ROSEMARY_PREFIX_HOME".to_string(), "prefix".to_string()),
        ("ROSEMARY_PREFIX_SETUP".to_string(), "download".to_string()),
    ];

    let metal = resolved_backend(settings) == Backend::Vulkan;
    if !metal {
        // hiding every Vulkan driver is Butter's "disable Metal": the guest
        // falls back to OpenGL.
        environment.push(("VK_DRIVER_FILES".to_string(), "/dev/null".to_string()));
    }
    let cards = gpu::list();
    let device = cards
        .iter()
        .find(|card| settings.graphics.device.as_deref() == Some(card.slot.as_str()));
    if metal {
        if let Some(card) = device {
            for (key, value) in card.vulkan_env() {
                environment.push((key.to_string(), value));
            }
        }
    }
    if let Some(card) = device {
        for (key, value) in card.opengl_env() {
            environment.push((key.to_string(), value));
        }
    }

    // the Mac programs' variables and Rosemary's go last, so user lines win.
    if !settings.environment.guest.is_empty() {
        environment.push((
            "ROSEMARY_ENVIRON".to_string(),
            settings.environment.guest.join("\n"),
        ));
    }
    for line in &settings.environment.runner {
        match line.split_once('=') {
            Some((name, value)) if valid_env_name(name) => {
                environment.push((name.to_string(), value.to_string()));
            }
            None if valid_env_name(line) => {
                if let Ok(value) = std::env::var(line) {
                    environment.push((line.clone(), value));
                }
            }
            _ => {}
        }
    }
    environment
}

/// a usable environment variable name.
pub fn valid_env_name(name: &str) -> bool {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// whether a raw line is `NAME=VALUE` or a bare `NAME`.
pub fn valid_env_line(line: &str) -> bool {
    match line.split_once('=') {
        Some((name, _)) => valid_env_name(name),
        None => valid_env_name(line),
    }
}

/// what the guest is opened with; the link goes under every name a Mac app
/// might read from NSUserDefaults.
pub fn guest_arguments(uri: Option<&str>, prefix: &Path) -> Vec<String> {
    let Some(uri) = uri else {
        return Vec::new();
    };
    let link = guest_url(uri, prefix);
    let mut arguments = Vec::new();
    for flag in ["-protocolString", "-protocolUrl", "-protocolUri"] {
        arguments.push(flag.to_string());
        arguments.push(link.clone());
    }
    arguments.push(link.clone());
    // the experience and the server, spelled out beside the canonical link.
    let ids = parse_launch(&link);
    if let Some(place_id) = ids.place_id {
        arguments.push("-placeId".to_string());
        arguments.push(place_id.to_string());
    }
    if let Some(job_id) = ids.job_id {
        arguments.push("-gameInstanceId".to_string());
        arguments.push(job_id);
    }
    arguments
}

/// what a launch URL names: experience and, when present, server.
#[derive(Debug, PartialEq)]
struct LaunchIds {
    place_id: Option<u64>,
    job_id: Option<String>,
}

/// ids a `roblox-player:` or `roblox://` link carries; browsers
/// percent-encode, so both spellings are read.
fn parse_launch(uri: &str) -> LaunchIds {
    let place_id = value_after(uri, "placeId").and_then(|rest| {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    });
    let job_id = ["gameInstanceId", "gameId"]
        .iter()
        .find_map(|key| value_after(uri, key).and_then(uuid_at))
        .or_else(|| uri.split("gameinfo:").nth(1).and_then(uuid_at));
    LaunchIds { place_id, job_id }
}

/// text after `key=`, tolerating the percent-encoded `%3D`.
fn value_after<'a>(haystack: &'a str, key: &str) -> Option<&'a str> {
    let start = haystack.find(key)? + key.len();
    let rest = &haystack[start..];
    rest.strip_prefix('=')
        .or_else(|| rest.strip_prefix("%3D"))
        .or_else(|| rest.strip_prefix("%3d"))
}

/// a UUID at the very start of `text`.
fn uuid_at(text: &str) -> Option<String> {
    let candidate: String = text
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
        .collect();
    (candidate.len() == 36).then_some(candidate)
}

/// a link stays as it is; a file becomes a `file:` URL on the guest's drive
/// or host mount.
pub fn guest_url(arg: &str, prefix: &Path) -> String {
    let path: Option<PathBuf> = if arg.starts_with('/') {
        Some(PathBuf::from(arg))
    } else if arg
        .get(..5)
        .is_some_and(|start| start.eq_ignore_ascii_case("file:"))
    {
        glib::filename_from_uri(arg).ok().map(|(path, _)| path)
    } else {
        None
    };
    let Some(path) = path else {
        return arg.to_string();
    };
    let drive = prefix.join("root");
    let guest = match path.strip_prefix(&drive) {
        Ok(rest) => Path::new("/").join(rest),
        Err(_) => Path::new("/Volumes/Host").join(path.strip_prefix("/").unwrap_or(&path)),
    };
    glib::filename_to_uri(&guest, None::<&str>)
        .map(|uri| uri.to_string())
        .unwrap_or_else(|_| arg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_keep_their_spelling() {
        let prefix = Path::new("/tmp/roseline-prefix");
        let link = "roblox-player:1+launchmode:play+gameinfo:abc";
        let arguments = guest_arguments(Some(link), prefix);
        assert_eq!(arguments.len(), 7);
        assert_eq!(arguments[0], "-protocolString");
        assert_eq!(arguments[1], link);
        assert_eq!(arguments[2], "-protocolUrl");
        assert_eq!(arguments[4], "-protocolUri");
        assert_eq!(arguments[6], link);
    }

    #[test]
    fn launch_ids_come_from_the_link() {
        let ids = parse_launch(
            "https://www.roblox.com/games/start?placeId=4483381587&gameInstanceId=e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b",
        );
        assert_eq!(ids.place_id, Some(4483381587));
        assert_eq!(
            ids.job_id.as_deref(),
            Some("e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b")
        );
    }

    #[test]
    fn deep_link_carries_the_server() {
        let ids = parse_launch(
            "roblox://experiences/start?placeId=4483381587&gameInstanceId=e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b",
        );
        assert_eq!(ids.place_id, Some(4483381587));
        assert_eq!(
            ids.job_id.as_deref(),
            Some("e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b")
        );
    }

    #[test]
    fn launch_ids_survive_percent_encoding() {
        let link = "roblox-player:1+launchmode:play+gameinfo:e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b+launchtime:1+placelauncherurl:https%3A%2F%2Fassetgame.roblox.com%2Fgame%2FPlaceLauncher.ashx%3Frequest%3DRequestGame%26placeId%3D123%26gameId%3De42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b";
        let ids = parse_launch(link);
        assert_eq!(ids.place_id, Some(123));
        assert_eq!(
            ids.job_id.as_deref(),
            Some("e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b")
        );
    }

    #[test]
    fn guest_arguments_carry_the_ids() {
        let prefix = Path::new("/tmp/p");
        let arguments = guest_arguments(
            Some(
                "https://www.roblox.com/games/start?placeId=99&gameInstanceId=e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b",
            ),
            prefix,
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair[0] == "-placeId" && pair[1] == "99")
        );
        assert!(arguments.windows(2).any(|pair| pair[0] == "-gameInstanceId"
            && pair[1] == "e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b"));
    }

    #[test]
    fn deep_links_are_passed_through() {
        let link = "roblox://experiences/start?placeId=4483381587&gameInstanceId=e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b";
        let arguments = guest_arguments(Some(link), Path::new("/tmp/p"));
        assert_eq!(arguments[1], link);
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair[0] == "-placeId" && pair[1] == "4483381587")
        );
        assert!(arguments.windows(2).any(|pair| pair[0] == "-gameInstanceId"
            && pair[1] == "e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b"));
    }

    #[test]
    fn runner_selection_follows_the_checked_release() {
        let mut settings = Settings::default();
        assert!(set_runner(&mut settings, "rosemary-v0.4.2-official"));
        assert_eq!(settings.runner.as_deref(), Some("rosemary-v0.4.2-official"));
        assert!(!set_runner(&mut settings, "rosemary-v0.4.2-official"));
        assert!(set_runner(&mut settings, "rosemary-v0.5.0-official"));
        assert_eq!(settings.runner.as_deref(), Some("rosemary-v0.5.0-official"));
    }

    #[test]
    fn waits_for_the_guest_process_group_to_drain() {
        use std::os::unix::process::CommandExt;

        let mut command = std::process::Command::new("sleep");
        command.arg("0.4").process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        // the leader is reaped in production before the wait starts; give
        // this child the same treatment once it has had time to exit.
        let reaper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(600));
            let _ = child.wait();
        });
        let started = std::time::Instant::now();

        wait_for_group(pid, std::time::Duration::from_secs(3));

        assert!(started.elapsed() >= std::time::Duration::from_millis(500));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        reaper.join().unwrap();
    }

    #[test]
    fn environment_lines_validate() {
        assert!(valid_env_line("ENABLE_SOMETHING=1"));
        assert!(valid_env_line("DISPLAY"));
        assert!(!valid_env_line("1BAD=x"));
        assert!(!valid_env_line("WITH SPACE=1"));
        assert!(!valid_env_line(""));
        assert!(!valid_env_line("=nothing"));
    }

    #[test]
    fn no_link_no_arguments() {
        assert!(guest_arguments(None, Path::new("/tmp/p")).is_empty());
    }

    #[test]
    fn files_map_through_the_host_mount_and_the_drive() {
        let prefix = Path::new("/data/bottle/prefix");
        assert_eq!(
            guest_url("/home/miles/x.txt", prefix),
            "file:///Volumes/Host/home/miles/x.txt"
        );
        assert_eq!(
            guest_url("/data/bottle/prefix/root/Users/x/f.txt", prefix),
            "file:///Users/x/f.txt"
        );
    }
}
