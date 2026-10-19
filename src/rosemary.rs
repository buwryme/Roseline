// SPDX-License-Identifier: GPL-3.0-or-later

//! running the installed Rosemary for this bottle's prefix.

use std::process::Command;

use anyhow::{Result, bail};

use crate::config::Paths;
use crate::settings::Settings;

fn command() -> Result<Command> {
    let Some((_, binary)) = Paths::get().runner() else {
        bail!("no Rosemary runner is installed yet");
    };
    Ok(Command::new(binary))
}

/// `rosemary prefix <action>` for the maintenance buttons; setup downloads
/// are allowed, so a repair never stops to ask.
pub fn prefix_command(action: &str) -> Result<Command> {
    let mut command = command()?;
    command
        .arg("prefix")
        .arg(action)
        .arg(Paths::get().prefix())
        .env("ROSEMARY_PREFIX_SETUP", "download");
    Ok(command)
}

/// `rosemary prefix create` for the bottle's settings.
pub fn create_command(settings: &Settings) -> Result<Command> {
    let mut command = command()?;
    command
        .arg("prefix")
        .arg("create")
        .arg(Paths::get().prefix())
        .arg("--arch")
        .arg("x86_64")
        .arg("--system-files")
        .arg(settings.prefix.system_files.value())
        .env("ROSEMARY_PREFIX_SETUP", "download");
    Ok(command)
}
