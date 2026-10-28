// SPDX-License-Identifier: GPL-3.0-or-later

//! the pre-2024 avatar editor background, restored by a client file mod.
//!
//! the modded Mobile.rbxl comes from Bloxstrap's repository; the stock file
//! is kept in a version-keyed backup so a toggle-off can put it back.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};

use crate::config::Paths;
use crate::progress::Progress;
use crate::settings::Settings;

/// one file carries the avatar editor's background.
const ROOT: &str = "Contents/Resources/ExtraContent/places/Mobile.rbxl";
const MARKER: &str = "version";
const MOD_CACHE: &str = "mod.rbxl";
const MOD_URL: &str = "https://raw.githubusercontent.com/bloxstraplabs/bloxstrap/main/Bloxstrap/Resources/Mods/OldAvatarBackground.rbxl";
const LABEL: &str = "the old avatar editor background";

pub fn apply(settings: &Settings, progress: &Progress) -> Result<()> {
    apply_at(&Paths::get(), settings, progress)
}

fn apply_at(paths: &Paths, settings: &Settings, progress: &Progress) -> Result<()> {
    let target = paths.app_bundle().join(ROOT);
    if !target.is_file() {
        return Ok(());
    }
    if settings.avatar_background.old {
        install(paths, &target, progress)
    } else {
        restore(paths, &target)
    }
}

fn install(paths: &Paths, target: &Path, progress: &Progress) -> Result<()> {
    let backup = paths.avatar_background();
    let version = crate::roblox::installed_upload_at(&paths.app_version()).unwrap_or_default();
    let mod_bytes = mod_bytes(paths, &version, progress)?;

    let current = fs::read(target).with_context(|| format!("reading {}", target.display()))?;
    if current == mod_bytes {
        return Ok(());
    }

    let original = backup.join(ROOT);
    if !keepable(&backup, &version, &current) {
        fs::create_dir_all(original.parent().context("the backup has no parent")?)?;
        fs::copy(target, &original).with_context(|| format!("backing up {}", target.display()))?;
        crate::util::write_atomic(&backup.join(MARKER), version.as_bytes())?;
    }

    crate::util::write_atomic(target, &mod_bytes)
        .with_context(|| format!("replacing {}", target.display()))?;
    fs::set_permissions(target, fs::Permissions::from_mode(0o644))?;
    Ok(())
}

/// a backup is kept when it exists for this version and holds the stock file.
fn keepable(backup: &Path, version: &str, current: &[u8]) -> bool {
    let Ok(stored) = fs::read_to_string(backup.join(MARKER)) else {
        return false;
    };
    stored == version
        && match fs::read(backup.join(ROOT)) {
            Ok(bytes) => bytes != current,
            Err(_) => false,
        }
}

fn restore(paths: &Paths, target: &Path) -> Result<()> {
    let backup = paths.avatar_background();
    let original = backup.join(ROOT);
    let bytes = fs::read(&original).with_context(|| format!("reading {}", original.display()))?;
    crate::util::write_atomic(target, &bytes)
        .with_context(|| format!("restoring {}", target.display()))?;
    fs::set_permissions(target, fs::Permissions::from_mode(0o644))?;
    fs::remove_file(&original)?;
    if fs::symlink_metadata(backup.join(MARKER)).is_ok() {
        fs::remove_file(backup.join(MARKER)).with_context(|| "removing the background marker")?;
    }
    prune_empty_dirs(&backup);
    Ok(())
}

/// the cached copy lives in the backup directory, so a toggle without a
/// network still has the mod available.
fn mod_bytes(paths: &Paths, version: &str, progress: &Progress) -> Result<Vec<u8>> {
    let backup = paths.avatar_background();
    let cache = backup.join(MOD_CACHE);
    if let Ok(bytes) = fs::read(&cache) {
        return Ok(bytes);
    }
    fs::create_dir_all(&backup)?;
    crate::util::write_atomic(&backup.join(MARKER), version.as_bytes())?;
    crate::net::download(MOD_URL, &cache, LABEL, progress)?;
    fs::set_permissions(&cache, fs::Permissions::from_mode(0o600))?;
    fs::read(&cache).with_context(|| format!("reading {}", cache.display()))
}

fn prune_empty_dirs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            prune_empty_dirs(&entry.path());
            let _ = fs::remove_dir(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> (std::path::PathBuf, Paths) {
        let root =
            std::env::temp_dir().join(format!("roseline-background-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            data: root.clone(),
            config: root.clone(),
            cache: root.clone(),
        };
        fs::create_dir_all(paths.app()).unwrap();
        fs::write(paths.app_version(), b"version-1").unwrap();
        (root, paths)
    }

    fn stock_file(paths: &Paths) -> std::path::PathBuf {
        let path = paths.app_bundle().join(ROOT);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        path
    }

    fn seed(paths: &Paths) -> std::path::PathBuf {
        fs::write(stock_file(paths), b"stock").unwrap();
        fs::create_dir_all(paths.avatar_background()).unwrap();
        fs::write(paths.avatar_background().join(MOD_CACHE), b"old").unwrap();
        stock_file(paths)
    }

    #[test]
    fn enabling_replaces_the_file_and_keeps_the_stock_one() {
        let (root, paths) = temp_paths("enable");
        let target = seed(&paths);
        let progress = Progress::silent();
        let mut settings = Settings::default();
        settings.avatar_background.old = true;

        apply_at(&paths, &settings, &progress).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert_eq!(
            fs::read(paths.avatar_background().join(ROOT)).unwrap(),
            b"stock"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn disabling_brings_the_stock_file_back() {
        let (root, paths) = temp_paths("disable");
        let target = seed(&paths);
        let progress = Progress::silent();
        let mut settings = Settings::default();
        settings.avatar_background.old = true;
        apply_at(&paths, &settings, &progress).unwrap();
        settings.avatar_background.old = false;

        apply_at(&paths, &settings, &progress).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"stock");
        assert!(!paths.avatar_background().join(ROOT).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_new_client_version_gets_a_fresh_backup() {
        let (root, paths) = temp_paths("version");
        let target = seed(&paths);
        let progress = Progress::silent();
        let mut settings = Settings::default();
        settings.avatar_background.old = true;
        apply_at(&paths, &settings, &progress).unwrap();

        fs::write(paths.app_bundle().join(ROOT), b"new-stock").unwrap();
        fs::write(paths.app_version(), b"version-2").unwrap();
        fs::write(paths.avatar_background().join(ROOT), b"old").unwrap();

        apply_at(&paths, &settings, &progress).unwrap();

        assert_eq!(
            fs::read(paths.avatar_background().join(ROOT)).unwrap(),
            b"new-stock"
        );
        assert_eq!(fs::read(&target).unwrap(), b"old");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn enabling_again_reuses_the_kept_backup() {
        let (root, paths) = temp_paths("again");
        let target = seed(&paths);
        let progress = Progress::silent();
        let mut settings = Settings::default();
        settings.avatar_background.old = true;
        apply_at(&paths, &settings, &progress).unwrap();
        settings.avatar_background.old = false;
        apply_at(&paths, &settings, &progress).unwrap();
        fs::write(&target, b"stock").unwrap();
        settings.avatar_background.old = true;

        apply_at(&paths, &settings, &progress).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert_eq!(
            fs::read(paths.avatar_background().join(ROOT)).unwrap(),
            b"stock"
        );
        let _ = fs::remove_dir_all(root);
    }
}
