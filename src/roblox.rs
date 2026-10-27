// SPDX-License-Identifier: GPL-3.0-or-later

//! the Roblox client: version endpoint, zip, and managed copy.

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::Paths;
use crate::progress::Progress;

const VERSION_URL: &str = "https://clientsettingscdn.roblox.com/v2/client-version/MacPlayer";

const CHANNEL_FILES: [&str; 2] = [
    "com.roblox.RobloxPlayerChannel.plist",
    "net.buwryy.RoselineChannel.plist",
];

const PRODUCTION_CHANNEL_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>www.roblox.com</key>
	<string></string>
</dict>
</plist>
"#;

#[derive(Debug, Clone, Deserialize)]
pub struct ClientVersion {
    pub version: String,
    /// `version-<hash>`, which names the download on setup.rbxcdn.com.
    #[serde(rename = "clientVersionUpload")]
    pub upload: String,
}

pub fn latest() -> Result<ClientVersion> {
    crate::net::get_json(VERSION_URL, &[]).context("could not read Roblox's version endpoint")
}

/// the `version-…` the managed copy came from.
pub fn installed_upload() -> Option<String> {
    installed_upload_at(&Paths::get().app_version())
}

pub(crate) fn installed_upload_at(version_file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(version_file).ok()?;
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// bring the managed copy up to Roblox's latest; returns whether it downloaded.
pub fn ensure(progress: &Progress) -> Result<bool> {
    let paths = Paths::get();
    remove_stray_app(&paths)?;
    disable_updater(&paths)?;
    let latest = latest()?;
    let bundle_ready = paths
        .app_bundle()
        .join("Contents/MacOS/RobloxPlayer")
        .is_file()
        && bundle_version(&paths.app_bundle()).as_deref() == Some(latest.version.as_str());
    if installed_upload().as_deref() == Some(latest.upload.as_str()) && bundle_ready {
        return Ok(false);
    }

    let url = format!(
        "https://setup.rbxcdn.com/mac/{}-RobloxPlayer.zip",
        latest.upload
    );
    let zip = paths
        .downloads()
        .join(format!("{}-RobloxPlayer.zip", latest.upload));
    progress.stage(format!("Downloading the Roblox client {}", latest.version));
    crate::net::download(&url, &zip, "Downloading the Roblox client", progress)?;

    progress.stage("Unpacking the Roblox client");
    let staging = paths.data.join("app.new");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    extract(&zip, &staging, progress)?;
    if paths.app().exists() {
        std::fs::remove_dir_all(paths.app())?;
    }
    std::fs::rename(&staging, paths.app())?;
    crate::util::write_atomic(&paths.app_version(), latest.upload.as_bytes())?;
    remove_stray_app(&paths)?;
    disable_updater(&paths)?;
    let _ = std::fs::remove_file(&zip);
    Ok(true)
}

fn bundle_version(bundle: &Path) -> Option<String> {
    const KEY: &str = "<key>CFBundleShortVersionString</key>";
    let text = std::fs::read_to_string(bundle.join("Contents/Info.plist")).ok()?;
    let after_key = &text[text.find(KEY)? + KEY.len()..];
    let open = after_key.find("<string>")? + "<string>".len();
    let close = after_key[open..].find("</string>")? + open;
    Some(after_key[open..close].trim().to_string())
}

fn remove_stray_app(paths: &Paths) -> Result<()> {
    let stray = paths.app().join("Roblox.app");
    remove_path(&stray)
}

fn disable_updater(paths: &Paths) -> Result<()> {
    let bundle = paths.app_bundle();
    remove_path(&bundle.join("Contents/MacOS/RobloxPlayerInstaller.app"))?;
    remove_path(&bundle.join("Contents/MacOS/RobloxPlayerInstaller"))?;
    if let Some(user) = paths.guest_user() {
        let library = user.join("Library/Roblox");
        for name in ["RobloxPlayer.zip", "RobloxPlayerInstaller", "versions"] {
            remove_path(&library.join(name))?;
        }
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("could not inspect {}", path.display()));
        }
    };
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("could not remove {}", path.display()))
    } else {
        std::fs::remove_file(path).with_context(|| format!("could not remove {}", path.display()))
    }
}

/// keep the client on the same production channel Roseline downloads from.
fn pin_production_channel(paths: &Paths) -> Result<()> {
    let Some(user) = paths.guest_user() else {
        return Ok(());
    };
    let preferences = user.join("Library/Preferences");
    for name in CHANNEL_FILES {
        let path = preferences.join(name);
        crate::util::write_atomic(&path, PRODUCTION_CHANNEL_PLIST.as_bytes())
            .with_context(|| format!("could not pin {}", path.display()))?;
    }
    Ok(())
}

/// the zip's app bundle, wherever it hides; some builds wrap it.
pub fn bundle_root(names: &[String]) -> Option<String> {
    let mut roots = Vec::new();
    for name in names {
        let parts: Vec<&str> = name.split('/').filter(|part| !part.is_empty()).collect();
        if let Some(index) = parts.iter().position(|part| part.ends_with(".app")) {
            let root = parts[..=index].join("/");
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    // prefer a candidate that carries the executable directory.
    let marker = |root: &String| format!("{root}/Contents/MacOS/");
    roots
        .iter()
        .find(|root| names.iter().any(|name| name.starts_with(&marker(root))))
        .cloned()
        .or_else(|| roots.first().cloned())
}

fn extract(zip_path: &Path, parent: &Path, progress: &Progress) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file).context("the client zip is unreadable")?;
    let names: Vec<String> = archive
        .file_names()
        .map(|name| name.map(|name| name.into_owned()))
        .collect::<std::result::Result<_, _>>()?;
    let root = bundle_root(&names).context("the client zip holds no app bundle")?;
    let prefix = format!("{root}/");
    let destination = parent.join("RobloxPlayer.app");
    std::fs::create_dir_all(&destination)?;

    let total = names.len().max(1);
    for (index, name) in names.iter().enumerate() {
        if index % 64 == 0 {
            progress.stage(format!("Unpacking the Roblox client ({index}/{total})"));
        }
        let Some(relative) = name.strip_prefix(&prefix) else {
            continue;
        };
        if relative.is_empty() || relative.split('/').any(|part| part == "..") {
            continue;
        }
        let target = destination.join(relative);
        let mut entry = archive.by_index(index)?;
        let mode = entry.unix_mode();
        if entry.is_dir() || name.ends_with('/') {
            std::fs::create_dir_all(&target)?;
        } else if mode.is_some_and(|mode| mode & 0o170000 == 0o120000) {
            let mut link = String::new();
            entry.read_to_string(&mut link)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = std::fs::remove_file(&target);
            std::os::unix::fs::symlink(link.trim(), &target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&target)?;
            std::io::copy(&mut entry, &mut out)?;
            if let Some(mode) = mode {
                let _ = std::fs::set_permissions(
                    &target,
                    std::fs::Permissions::from_mode(mode & 0o777),
                );
            }
        }
    }
    Ok(())
}

/// point the bundle identifier at Roseline, which becomes the window class
/// the compositor sees. the identifier also keys cookies and preferences,
/// so those files move with it and the login survives.
pub fn brand() -> Result<()> {
    let paths = Paths::get();
    let plist = paths.app_bundle().join("Contents/Info.plist");
    if let Ok(text) = std::fs::read_to_string(&plist)
        && let Some(patched) = rewrite_identifier(&text, crate::config::APP_ID)
    {
        crate::util::write_atomic(&plist, patched.as_bytes())?;
    }
    if let Some(user) = paths.guest_user() {
        for directory in ["Library/Cookies", "Library/Preferences"] {
            let root = user.join(directory);
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.flatten() {
                let Some(branded) = branded_name(&entry.file_name().to_string_lossy()) else {
                    continue;
                };
                let target = root.join(&branded);
                if !target.exists() {
                    let _ = std::fs::copy(entry.path(), &target);
                }
            }
        }
    }
    pin_production_channel(&paths)?;
    Ok(())
}

/// `com.roblox.RobloxPlayerChannel.plist` → `net.buwryy.RoselineChannel.plist`.
fn branded_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix("com.roblox.RobloxPlayer")?;
    Some(format!("{}{}", crate::config::APP_ID, rest))
}

/// the `<string>` after the identifier key, rewritten in place; `None` when
/// already right.
fn rewrite_identifier(text: &str, id: &str) -> Option<String> {
    const KEY: &str = "<key>CFBundleIdentifier</key>";
    let key_at = text.find(KEY)? + KEY.len();
    let open = text[key_at..].find("<string>")? + key_at + "<string>".len();
    let close = text[open..].find("</string>")? + open;
    if &text[open..close] == id {
        return None;
    }
    let mut patched = String::with_capacity(text.len() + id.len());
    patched.push_str(&text[..open]);
    patched.push_str(id);
    patched.push_str(&text[close..]);
    Some(patched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_updater_and_its_payload_are_removed() {
        let root = std::env::temp_dir().join(format!("roseline-updater-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths {
            data: root.clone(),
            config: root.clone(),
            cache: root.clone(),
        };
        let installer = paths
            .app_bundle()
            .join("Contents/MacOS/RobloxPlayerInstaller.app/Contents/MacOS");
        std::fs::create_dir_all(&installer).unwrap();
        std::fs::write(installer.join("RobloxPlayerInstaller"), b"x").unwrap();
        std::fs::create_dir_all(paths.app().join("Roblox.app")).unwrap();
        let guest = paths.prefix().join("root/Users/miles/Library/Roblox");
        std::fs::create_dir_all(&guest).unwrap();
        std::fs::write(guest.join("RobloxPlayer.zip"), b"x").unwrap();
        std::fs::create_dir_all(guest.join("RobloxPlayerInstaller")).unwrap();

        remove_stray_app(&paths).unwrap();
        disable_updater(&paths).unwrap();

        assert!(!paths.app().join("Roblox.app").exists());
        assert!(
            !paths
                .app_bundle()
                .join("Contents/MacOS/RobloxPlayerInstaller.app")
                .exists()
        );
        assert!(!guest.join("RobloxPlayer.zip").exists());
        assert!(!guest.join("RobloxPlayerInstaller").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn finds_the_bundle_at_the_zip_root() {
        let names = vec![
            "RobloxPlayer.app/".to_string(),
            "RobloxPlayer.app/Contents/Info.plist".to_string(),
            "RobloxPlayer.app/Contents/MacOS/RobloxPlayer".to_string(),
        ];
        assert_eq!(bundle_root(&names).as_deref(), Some("RobloxPlayer.app"));
    }

    #[test]
    fn finds_a_wrapped_bundle() {
        let names = vec![
            "Release-RobloxPlayer/RobloxPlayer.app/Contents/Info.plist".to_string(),
            "Release-RobloxPlayer/RobloxPlayer.app/Contents/MacOS/RobloxPlayer".to_string(),
            "Release-RobloxPlayer/README.txt".to_string(),
        ];
        assert_eq!(
            bundle_root(&names).as_deref(),
            Some("Release-RobloxPlayer/RobloxPlayer.app")
        );
    }

    #[test]
    fn no_bundle_is_none() {
        assert_eq!(bundle_root(&["plain.txt".to_string()]), None);
    }

    #[test]
    fn client_version_json_parses() {
        let parsed: ClientVersion = serde_json::from_str(
            r#"{"version":"0.700.1","clientVersionUpload":"version-212bac6bdccd4673"}"#,
        )
        .unwrap();
        assert_eq!(parsed.upload, "version-212bac6bdccd4673");
    }

    #[test]
    fn identifiers_are_rewritten_in_place() {
        let text = "\t<key>CFBundleIdentifier</key>\n\t<string>com.roblox.RobloxPlayer</string>\n";
        let patched = rewrite_identifier(text, "net.buwryy.Roseline").unwrap();
        assert!(patched.contains("<string>net.buwryy.Roseline</string>"));
        assert!(rewrite_identifier(&patched, "net.buwryy.Roseline").is_none());
    }

    #[test]
    fn state_files_follow_the_identifier() {
        assert_eq!(
            branded_name("com.roblox.RobloxPlayer.cookies.tsv").as_deref(),
            Some("net.buwryy.Roseline.cookies.tsv")
        );
        assert_eq!(
            branded_name("com.roblox.RobloxPlayerChannel.plist").as_deref(),
            Some("net.buwryy.RoselineChannel.plist")
        );
        assert_eq!(branded_name("something-else"), None);
    }

    #[test]
    fn reads_the_version_from_the_bundle_plist() {
        let root = std::env::temp_dir().join(format!("roseline-plist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let contents = root.join("Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            "<plist><dict><key>CFBundleShortVersionString</key>\n<string>0.744.0.7441080</string></dict></plist>",
        )
        .unwrap();

        assert_eq!(bundle_version(&root).as_deref(), Some("0.744.0.7441080"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn removes_a_stray_unbranded_bundle() {
        let root = std::env::temp_dir().join(format!("roseline-stray-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths {
            data: root.clone(),
            config: root.clone(),
            cache: root.clone(),
        };
        let stray = paths.app().join("Roblox.app");
        std::fs::create_dir_all(stray.join("Contents/MacOS")).unwrap();
        std::fs::write(stray.join("Contents/MacOS/RobloxPlayer"), b"old").unwrap();

        remove_stray_app(&paths).unwrap();

        assert!(!stray.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pins_both_channel_preferences_to_production() {
        let root = std::env::temp_dir().join(format!("roseline-channel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths {
            data: root.clone(),
            config: root.clone(),
            cache: root.clone(),
        };
        let preferences = paths.prefix().join("root/Users/miles/Library/Preferences");
        std::fs::create_dir_all(&preferences).unwrap();
        for name in CHANNEL_FILES {
            std::fs::write(preferences.join(name), "zbeta").unwrap();
        }

        pin_production_channel(&paths).unwrap();

        for name in CHANNEL_FILES {
            let text = std::fs::read_to_string(preferences.join(name)).unwrap();
            assert!(!text.contains("zbeta"));
            assert!(text.contains("<string></string>"));
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
