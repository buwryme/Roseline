// SPDX-License-Identifier: GPL-3.0-or-later

//! application constants and on-disk locations.

use std::path::PathBuf;

pub const APP_ID: &str = "net.buwryy.Roseline";
/// display name; ids, paths and user agents keep the plain spelling.
pub const DISPLAY_NAME: &str = "Roséline";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const WEBSITE: &str = "https://github.com/buwryme/Roseline";
/// https page that hands a Discord join button to the browser.
pub const JOIN_PAGE: &str = "https://buwryy.net/join-roseline-discord-rpc-handler.html";

/// where the runners come from.
pub const ROSEMARY_OWNER: &str = "microlift";
pub const ROSEMARY_REPO: &str = "rosemary";

pub fn user_agent() -> String {
    format!("Roseline/{VERSION} (+{WEBSITE})")
}

/// where Roseline keeps everything; GLib redirects these inside a Flatpak.
#[derive(Debug, Clone)]
pub struct Paths {
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
}

impl Paths {
    pub fn get() -> Paths {
        Paths {
            data: gtk::glib::user_data_dir().join("roseline"),
            config: gtk::glib::user_config_dir().join("roseline"),
            cache: gtk::glib::user_cache_dir().join("roseline"),
        }
    }

    /// one directory per installed Rosemary build.
    pub fn runners(&self) -> PathBuf {
        self.data.join("runners")
    }

    /// the single bottle: prefix, logs and run state.
    pub fn bottle(&self) -> PathBuf {
        self.data.join("bottle")
    }

    pub fn prefix(&self) -> PathBuf {
        self.bottle().join("prefix")
    }

    pub fn logs(&self) -> PathBuf {
        self.bottle().join("logs")
    }

    /// pid of the running game, kept across restarts of Roseline.
    pub fn game_pid(&self) -> PathBuf {
        self.bottle().join("game.pid")
    }

    /// the managed client copy and its state files.
    pub fn app(&self) -> PathBuf {
        self.data.join("app")
    }

    pub fn app_bundle(&self) -> PathBuf {
        self.app().join("RobloxPlayer.app")
    }

    /// the `version-…` upload the copy came from.
    pub fn app_version(&self) -> PathBuf {
        self.app().join("version")
    }

    /// what the overlay last wrote, for the skip check.
    pub fn overlay_state(&self) -> PathBuf {
        self.app().join("overlay-state.toml")
    }

    /// content overrides, mirroring paths inside the app bundle.
    pub fn overlay(&self) -> PathBuf {
        self.data.join("overlay")
    }

    /// the client's own font files, kept while a font choice is applied.
    pub fn font_backup(&self) -> PathBuf {
        self.data.join("font-backup")
    }

    /// the stock avatar background and the mod that replaces it.
    pub fn avatar_background(&self) -> PathBuf {
        self.data.join("avatar-background")
    }

    /// imported copies of the fonts the user picked.
    pub fn font_store(&self) -> PathBuf {
        self.data.join("fonts")
    }

    pub fn downloads(&self) -> PathBuf {
        self.cache.join("downloads")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    /// the newest installed runner and its binary.
    pub fn runner(&self) -> Option<(String, PathBuf)> {
        let mut entries: Vec<(Vec<u64>, String, PathBuf)> = std::fs::read_dir(self.runners())
            .ok()?
            .flatten()
            .filter_map(|entry| {
                let id = entry.file_name().to_string_lossy().into_owned();
                let tag = crate::runner::tag_of(&id)?;
                let binary = entry.path().join("bin/rosemary");
                binary
                    .is_file()
                    .then(|| (crate::runner::version_key(&tag), id, binary))
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries.pop().map(|(_, id, binary)| (id, binary))
    }

    pub fn runner_by_id(&self, id: &str) -> Option<(String, PathBuf)> {
        let binary = self.runners().join(id).join("bin/rosemary");
        binary.is_file().then(|| (id.to_string(), binary))
    }

    /// the guest's home directory; a prefix normally has exactly one.
    pub fn guest_user(&self) -> Option<PathBuf> {
        let users = self.prefix().join("root/Users");
        std::fs::read_dir(users)
            .ok()?
            .flatten()
            .find(|entry| entry.path().is_dir())
            .map(|entry| entry.path())
    }

    /// the pinned runner when installed, the newest otherwise.
    pub fn runner_selected(&self, pinned: Option<&str>) -> Option<(String, PathBuf)> {
        pinned
            .and_then(|id| self.runner_by_id(id))
            .or_else(|| self.runner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_picks_the_newest_version_numerically() {
        let root = std::env::temp_dir().join(format!("roseline-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths {
            data: root.clone(),
            config: root.clone(),
            cache: root.clone(),
        };
        for id in [
            "rosemary-v0.9.0-official",
            "rosemary-v0.10.0-official",
            "rosemary-v0.2.1-official",
        ] {
            let bin = paths.runners().join(id).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("rosemary"), b"#!/bin/sh\n").unwrap();
        }
        let (id, _) = paths.runner().unwrap();
        assert_eq!(id, "rosemary-v0.10.0-official");
        assert_eq!(
            paths
                .runner_selected(Some("rosemary-v0.2.1-official"))
                .unwrap()
                .0,
            "rosemary-v0.2.1-official"
        );
        assert_eq!(
            paths.runner_selected(Some("missing")).unwrap().0,
            "rosemary-v0.10.0-official"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
