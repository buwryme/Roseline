// SPDX-License-Identifier: GPL-3.0-or-later

//! installing official Rosemary builds from its GitHub releases.
//!
//! unpacking follows Butter's `runner.rs` (GPL-3.0-or-later, the Microlift
//! contributors).

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::config::Paths;
use crate::github::Release;
use crate::progress::Progress;

const BINARY_LAUNCHER: &str = r#"#!/bin/sh
# runs a copy of the rosemary build named Roseline, and points the bundled
# loader at a copy of itself with that name too, so tools that match on the
# executable (vulkan layers, mangohud) see Roseline.
set -eu

if [ -n "${STEAM_RUNTIME:-}" ]; then
    LD_LIBRARY_PATH=${SYSTEM_LD_LIBRARY_PATH:-}
    [ -n "${SYSTEM_PATH:-}" ] && PATH=$SYSTEM_PATH
    [ -n "$LD_LIBRARY_PATH" ] || unset LD_LIBRARY_PATH
    unset LD_PRELOAD STEAM_RUNTIME_LIBRARY_PATH
fi

here=${0%/*}
root=$here/../Rosemary
loader=$root/glibc/lib/ld-linux-x86-64.so.2
named_loader=$root/glibc/lib/Roseline

# a runner update restores the real loader, so redo this every launch
if [ -L "$loader" ] && [ ! -f "$named_loader" ]; then
    rm -f "$loader"
fi
if [ ! -L "$loader" ] && [ -f "$loader" ]; then
    cp -f "$loader" "$named_loader"
    chmod 755 "$named_loader"
    rm -f "$loader"
    ln -s Roseline "$loader"
fi

launcher=$root/bin/Roseline
if [ ! -x "$launcher" ] || ! cmp -s "$root/bin/rosemary" "$launcher"; then
    cp -f "$root/bin/rosemary" "$launcher"
    chmod 755 "$launcher"
fi
exec "$launcher" "$@"
"#;

#[derive(Serialize)]
struct Info {
    name: String,
    kind: &'static str,
    tag: String,
    executable: &'static str,
    installed: u64,
}

/// directory name a release tag installs as.
pub fn id_for(tag: &str) -> String {
    format!("rosemary-{tag}-official")
}

/// release tag an installed runner id carries.
pub(crate) fn tag_of(id: &str) -> Option<String> {
    id.strip_prefix("rosemary-")?
        .strip_suffix("-official")
        .map(str::to_string)
}

/// installed runners, newest first.
pub fn installed() -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(Paths::get().runners()) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            let tag = tag_of(&id)?;
            entry
                .path()
                .join("bin/rosemary")
                .is_file()
                .then_some((id, tag))
        })
        .collect();
    found.sort_by(|a, b| version_key(&a.1).cmp(&version_key(&b.1)));
    found.reverse();
    found
}

pub fn version_key(tag: &str) -> Vec<u64> {
    tag.trim_start_matches('v')
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// install the latest stable build unless it is already here.
pub fn ensure(progress: &Progress) -> Result<String> {
    progress.stage("Checking for the latest Rosemary release");
    let release = crate::github::latest_release()?;
    let id = id_for(&release.tag_name);
    if Paths::get()
        .runners()
        .join(&id)
        .join("bin/rosemary")
        .is_file()
    {
        return Ok(id);
    }
    install(&release, progress)
}

/// download, verify and unpack one release; older versions stay pinnable.
pub fn install(release: &Release, progress: &Progress) -> Result<String> {
    let paths = Paths::get();
    let id = id_for(&release.tag_name);
    let dir = paths.runners().join(&id);
    if dir.join("bin/rosemary").is_file() {
        return Ok(id);
    }

    let asset = release.steam_tool().with_context(|| {
        format!(
            "Rosemary {} has no official build for this architecture",
            release.tag_name
        )
    })?;
    let expected = match asset.sha256() {
        Some(hash) => hash,
        None => {
            let sums = release.asset("SHA256SUMS").with_context(|| {
                format!(
                    "Rosemary {} publishes no checksum for {}",
                    release.tag_name, asset.name
                )
            })?;
            let text = crate::net::get_text(&sums.browser_download_url, &[])?;
            crate::github::checksum_for(&text, &asset.name)
                .with_context(|| format!("SHA256SUMS does not list {}", asset.name))?
        }
    };

    let archive = paths.downloads().join(&asset.name);
    crate::net::download(
        &asset.browser_download_url,
        &archive,
        &format!("Downloading {}", release.title()),
        progress,
    )?;
    progress.stage("Verifying the download");
    let actual = crate::net::sha256_file(&archive)?;
    if actual != expected {
        let _ = std::fs::remove_file(&archive);
        bail!("the download is damaged: its SHA-256 is {actual}, but the release lists {expected}");
    }

    unpack_tool(&archive, &dir)?;
    write_launcher(&dir)?;
    let info = Info {
        name: format!(
            "Rosemary {} (Official Build)",
            release.tag_name.trim_start_matches('v')
        ),
        kind: "binary",
        tag: release.tag_name.clone(),
        executable: "bin/rosemary",
        installed: now(),
    };
    crate::util::write_atomic(
        &dir.join("runner.toml"),
        toml::to_string_pretty(&info)?.as_bytes(),
    )?;
    let _ = std::fs::remove_file(&archive);
    Ok(id)
}

/// rewrite the launcher, so an updated script reaches installed runners.
pub fn refresh_launcher(dir: &Path) -> Result<()> {
    write_launcher(dir)
}

fn write_launcher(dir: &Path) -> Result<()> {
    let path = dir.join("bin/rosemary");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, BINARY_LAUNCHER)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// unpack a Steam tool into `dest`; entries that would land outside it are
/// refused, symbolic links included.
fn unpack_tool(archive: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let file = std::fs::File::open(archive)?;
    let reader: Box<dyn Read> = if archive
        .extension()
        .is_some_and(|extension| extension == "zst")
    {
        Box::new(zstd::stream::read::Decoder::new(file)?)
    } else {
        Box::new(flate2::read::GzDecoder::new(file))
    };
    let mut tar = tar::Archive::new(reader);
    tar.set_preserve_permissions(true);
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if !entry.unpack_in(dest)? {
            bail!("refusing to unpack {}", path.display());
        }
    }
    if !dest.join("Rosemary/bin/rosemary").is_file() {
        bail!("the download has no Rosemary/bin/rosemary; it is not a Rosemary Steam tool");
    }
    Ok(())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_tags() {
        assert_eq!(id_for("v0.3.0"), "rosemary-v0.3.0-official");
        assert_eq!(
            tag_of("rosemary-v0.3.0-official").as_deref(),
            Some("v0.3.0")
        );
        assert_eq!(tag_of("something-else"), None);
    }

    #[test]
    fn versions_sort_numerically() {
        let mut tags = ["v0.9.0", "v0.10.0", "v0.3.0"];
        tags.sort_by_key(|tag| version_key(tag));
        assert_eq!(tags, ["v0.3.0", "v0.9.0", "v0.10.0"]);
    }
}
