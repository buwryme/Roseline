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
    let latest = latest()?;
    if installed_upload().as_deref() == Some(latest.upload.as_str())
        && paths.app_bundle().join("Contents/MacOS").is_dir()
    {
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
    let _ = std::fs::remove_file(&zip);
    Ok(true)
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
}
