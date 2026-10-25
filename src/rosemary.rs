// SPDX-License-Identifier: GPL-3.0-or-later

//! running the installed Rosemary for this bottle's prefix.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use anyhow::{Context, Result, bail};

use crate::config::Paths;
use crate::settings::Settings;

fn binary(settings: &Settings) -> Result<PathBuf> {
    let Some((_, binary)) = Paths::get().runner_selected(settings.runner.as_deref()) else {
        bail!("no Rosemary runner is installed yet");
    };
    Ok(binary)
}

/// the runner directory a `bin/rosemary` launcher belongs to.
pub fn runner_dir(binary: &Path) -> Result<&Path> {
    binary
        .parent()
        .and_then(Path::parent)
        .context("the Rosemary runner has no directory")
}

/// run `rosemary <arguments>` to the end, its output kept in `log`; setup
/// downloads are allowed, so a repair never stops to ask.
fn run(settings: &Settings, arguments: Vec<OsString>, log: &Path) -> Result<ExitStatus> {
    let binary = binary(settings)?;
    crate::runner::refresh_launcher(runner_dir(&binary)?)?;
    let environment = [("ROSEMARY_PREFIX_SETUP".to_string(), "download".to_string())];
    if crate::muvm::route(runner_dir(&binary)?, settings.compatibility.muvm)? {
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let directory = Paths::get().data;
        let mut command = crate::muvm::command(
            &binary,
            &arguments,
            &environment,
            &directory,
            log,
            settings.compatibility.vm_memory,
        )?;
        let output = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)?;
        command
            .stdin(std::process::Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output);
        return Ok(command.status()?);
    }
    let mut command = Command::new(binary);
    command.args(arguments).envs(environment);
    crate::util::run_logged(&mut command, log)
}

/// `rosemary prefix <action>` for the maintenance buttons.
pub fn prefix(action: &str, log: &Path) -> Result<ExitStatus> {
    run(
        &Settings::load(),
        vec!["prefix".into(), action.into(), Paths::get().prefix().into()],
        log,
    )
}

/// The base matches Roblox's x86_64 guest, including when an ARM64 runner
/// translates it. The runner's ELF architecture does not name the guest ABI.
pub fn create(settings: &Settings, log: &Path) -> Result<ExitStatus> {
    run(
        settings,
        vec![
            "prefix".into(),
            "create".into(),
            Paths::get().prefix().into(),
            "--arch".into(),
            "x86_64".into(),
            "--system-files".into(),
            settings.prefix.system_files.value().into(),
        ],
        log,
    )
}
