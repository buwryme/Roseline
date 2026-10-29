// SPDX-License-Identifier: GPL-3.0-or-later

//! installing official Rosemary builds from its GitHub releases.
//!
//! unpacking follows Butter's `runner.rs` (GPL-3.0-or-later, the Microlift
//! contributors).

use std::fs;
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
loader=$root/glibc/lib/@LOADER@
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

/// whether an id names a user import rather than an official release.
pub fn is_custom(id: &str) -> bool {
    id.starts_with("custom-")
}

/// installed runners, newest first.
pub fn installed() -> Vec<(String, String)> {
    installed_at(&Paths::get())
}

fn installed_at(paths: &Paths) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(paths.runners()) else {
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

/// user-imported runners, by name.
pub fn customs() -> Vec<(String, String)> {
    customs_at(&Paths::get())
}

fn customs_at(paths: &Paths) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(paths.runners()) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            if !is_custom(&id) {
                return None;
            }
            let name = id.trim_start_matches("custom-").to_string();
            paths
                .runners()
                .join(&id)
                .join("bin/rosemary")
                .is_file()
                .then_some((id, name))
        })
        .collect();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    found
}

/// a name is free when no runner directory, official tag or import uses it.
pub fn name_taken(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || name_taken_at(&Paths::get(), trimmed)
}

fn name_taken_at(paths: &Paths, name: &str) -> bool {
    let trimmed = name.trim();
    paths.runners().join(format!("custom-{trimmed}")).exists()
        || paths.runners().join(trimmed).exists()
        || customs_at(paths).iter().any(|(_, other)| other == trimmed)
        || installed_at(paths).iter().any(|(_, tag)| tag == trimmed)
}

/// bring a user's build in as a custom runner, pinned by the caller.
pub fn import_custom(source: &Path, name: &str) -> Result<String> {
    import_custom_at(&Paths::get(), source, name)
}

fn import_custom_at(paths: &Paths, source: &Path, name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("the name cannot be empty");
    }
    if trimmed == "." || trimmed == ".." || trimmed.contains('/') || trimmed.contains('\\') {
        bail!("the name must be a plain folder name");
    }
    if name_taken_at(paths, trimmed) {
        bail!("a runner named {trimmed} already exists");
    }
    if source.join("Rosemary/bin/rosemary").is_file() {
        let dir = paths.runners().join(custom_id_for(trimmed));
        finish_import(source, &dir, &dir, trimmed)
    } else if source.join("bin/rosemary").is_file() && source.join("glibc/lib").is_dir() {
        let dir = paths.runners().join(custom_id_for(trimmed));
        let embedded = dir.join("Rosemary");
        finish_import(source, &dir, &embedded, trimmed)
    } else {
        bail!("that folder has no Rosemary build inside")
    }
}

/// copy into `embedded`, write the launcher into `dir`, tag it; a failure
/// backs the copy out, so the name is free for the next attempt.
fn finish_import(source: &Path, dir: &Path, embedded: &Path, name: &str) -> Result<String> {
    let id = custom_id_for(name);
    if let Err(error) = copy_recursive(source, embedded)
        .and_then(|()| write_launcher(dir))
        .and_then(|()| write_custom_info(dir, name))
    {
        let _ = fs::remove_dir_all(dir);
        return Err(error);
    }
    Ok(id)
}

fn custom_id_for(name: &str) -> String {
    format!("custom-{name}")
}

fn write_custom_info(dir: &Path, name: &str) -> Result<()> {
    let info = Info {
        name: name.to_string(),
        kind: "custom",
        tag: String::new(),
        executable: "bin/rosemary",
        installed: now(),
    };
    crate::util::write_atomic(
        &dir.join("runner.toml"),
        toml::to_string_pretty(&info)?.as_bytes(),
    )?;
    Ok(())
}

fn copy_recursive(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(source)?.flatten() {
        let from = entry.path();
        let to = dest.join(entry.file_name());
        match entry.file_type()? {
            kind if kind.is_dir() => copy_recursive(&from, &to)?,
            kind if kind.is_symlink() => copy_link(&from, &to)?,
            _ => {
                fs::copy(&from, &to).with_context(|| format!("copying {}", from.display()))?;
            }
        }
    }
    Ok(())
}

/// a static copy keeps the name and puts the pointed-at data behind it;
/// runner builds ship .so links, so refusing them would break the build.
fn copy_link(from: &Path, to: &Path) -> Result<()> {
    let target = fs::read_link(from).with_context(|| format!("reading {}", from.display()))?;
    let resolved = match target.is_absolute() {
        true => target,
        false => from
            .parent()
            .with_context(|| format!("{} has no parent", from.display()))?
            .join(target),
    };
    let metadata = fs::metadata(&resolved)
        .with_context(|| format!("the link {} points at nothing", from.display()))?;
    if metadata.is_dir() {
        copy_recursive(&resolved, to)
    } else {
        fs::copy(&resolved, to).with_context(|| format!("copying {}", resolved.display()))?;
        Ok(())
    }
}

/// install the latest stable build unless it is already here.
pub fn ensure(progress: &Progress) -> Result<String> {
    ensure_at(&Paths::get(), progress)
}

fn ensure_at(paths: &Paths, progress: &Progress) -> Result<String> {
    progress.stage("Checking for the latest Rosemary release");
    match crate::github::latest_release() {
        Ok(release) => {
            remember_latest(paths, &release.tag_name);
            let id = id_for(&release.tag_name);
            if paths.runners().join(&id).join("bin/rosemary").is_file() {
                return Ok(id);
            }
            install(&release, progress)
        }
        Err(error) => {
            // a rate limit or an outage must not stop a launch; what is
            // installed keeps working, and the cached tag says what the
            // newest build was last time the API answered.
            match cached_id(paths) {
                Some(id) => {
                    eprintln!(
                        "roseline: could not check for a Rosemary update ({error:#}); launching with {id}"
                    );
                    Ok(id)
                }
                None => bail!(
                    "no Rosemary runner is installed, and checking the release failed: {error:#}"
                ),
            }
        }
    }
}

/// the id of the cached latest tag when it is still installed, else the
/// newest installed official.
fn cached_id(paths: &Paths) -> Option<String> {
    if let Some(tag) = cached_latest(paths) {
        let id = id_for(&tag);
        if paths.runners().join(&id).join("bin/rosemary").is_file() {
            return Some(id);
        }
    }
    let (id, _) = installed_at(paths).last()?.clone();
    Some(id)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Latest {
    tag: String,
}

fn cached_latest(paths: &Paths) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(paths.rosemary_cache())
        && let Ok(latest) = toml::from_str::<Latest>(&text)
    {
        return Some(latest.tag);
    }
    None
}

fn remember_latest(paths: &Paths, tag: &str) {
    let latest = Latest {
        tag: tag.to_string(),
    };
    if let Ok(text) = toml::to_string(&latest) {
        let _ = crate::util::write_atomic(&paths.rosemary_cache(), text.as_bytes());
    }
}

/// install one chosen release unless it is already here.
pub fn ensure_tag(progress: &Progress, tag: &str) -> Result<String> {
    let id = id_for(tag);
    if Paths::get()
        .runners()
        .join(&id)
        .join("bin/rosemary")
        .is_file()
    {
        return Ok(id);
    }
    progress.stage(format!("Finding Rosemary {tag}"));
    let release = crate::github::releases()?
        .into_iter()
        .find(|release| release.tag_name == tag)
        .with_context(|| format!("Rosemary {tag} vanished from GitHub"))?;
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

    let asset = match release.steam_tool() {
        Some(asset) => asset,
        None => emulated_tool(release)?,
    };
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

/// the x86_64 build, for an aarch64 host that runs it through muvm.
fn emulated_tool(release: &Release) -> Result<&crate::github::Asset> {
    let arch = std::env::consts::ARCH;
    let foreign = (arch == "aarch64")
        .then(|| release.steam_tool_for("x86_64"))
        .flatten();
    let Some(asset) = foreign else {
        bail!(
            "Rosemary {} has no official build for {arch}",
            release.tag_name
        );
    };
    if !crate::settings::Settings::load().compatibility.muvm {
        bail!(
            "Rosemary {} has no native ARM64 build and ARM64 compatibility is disabled; enable it in Advanced settings to use muvm",
            release.tag_name
        );
    }
    if crate::rpc::sandboxed() {
        bail!("{}", crate::muvm::missing_hint());
    }
    if !crate::muvm::available() {
        bail!(
            "Rosemary {} has no {arch} build, and its x86_64 one runs through muvm; {}",
            release.tag_name,
            crate::muvm::missing_hint()
        );
    }
    Ok(asset)
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
    let arch = crate::muvm::runner_arch(dir).unwrap_or(std::env::consts::ARCH);
    std::fs::write(&path, launcher_script(arch))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// the launcher with the runner's glibc loader filled in.
fn launcher_script(arch: &str) -> String {
    BINARY_LAUNCHER.replace("@LOADER@", loader_name(arch))
}

/// the dynamic loader Rosemary's bundled glibc ships for a runner of `arch`;
/// under muvm that is not the host's.
fn loader_name(arch: &str) -> &'static str {
    match arch {
        "aarch64" => "ld-linux-aarch64.so.1",
        _ => "ld-linux-x86-64.so.2",
    }
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
    fn launcher_names_the_runners_loader() {
        for (arch, loader) in [
            ("x86_64", "ld-linux-x86-64.so.2"),
            ("aarch64", "ld-linux-aarch64.so.1"),
        ] {
            let script = launcher_script(arch);
            assert!(!script.contains("@LOADER@"));
            assert!(script.contains(&format!("loader=$root/glibc/lib/{loader}\n")));
        }
    }

    #[test]
    fn versions_sort_numerically() {
        let mut tags = ["v0.9.0", "v0.10.0", "v0.3.0"];
        tags.sort_by_key(|tag| version_key(tag));
        assert_eq!(tags, ["v0.3.0", "v0.9.0", "v0.10.0"]);
    }

    fn temp_paths(tag: &str) -> (std::path::PathBuf, Paths) {
        let root =
            std::env::temp_dir().join(format!("roseline-runner-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        (
            root.clone(),
            Paths {
                data: root.clone(),
                config: root.clone(),
                cache: root,
            },
        )
    }

    fn a_build(source: &std::path::Path) {
        std::fs::create_dir_all(source.join("Rosemary/bin")).unwrap();
        std::fs::create_dir_all(source.join("Rosemary/glibc/lib")).unwrap();
        std::fs::write(source.join("Rosemary/bin/rosemary"), b"#!/bin/sh\n").unwrap();
        std::fs::write(source.join("Rosemary/glibc/lib/ld.so"), b"binary").unwrap();
    }

    #[test]
    fn an_import_lands_in_its_own_directory_with_a_launcher() {
        let (_root, paths) = temp_paths("import");
        let source = _root.join("download");
        a_build(&source);

        let id = import_custom_at(&paths, &source, "Mine").unwrap();

        assert_eq!(id, "custom-Mine");
        let dir = paths.runners().join(&id);
        assert!(dir.join("Rosemary/bin/rosemary").is_file());
        assert!(dir.join("Rosemary/glibc/lib/ld.so").is_file());
        let script = std::fs::read_to_string(dir.join("bin/rosemary")).unwrap();
        assert!(script.starts_with("#!"));
        assert!(!script.contains("@LOADER@"));
        let info = std::fs::read_to_string(dir.join("runner.toml")).unwrap();
        assert!(info.contains("kind = \"custom\""));
        assert!(info.contains("name = \"Mine\""));
        assert_eq!(customs_at(&paths), vec![(id.clone(), "Mine".to_string())]);
        let _ = std::fs::remove_dir_all(_root);
    }

    #[test]
    fn a_bare_build_gets_the_official_layout() {
        let (_root, paths) = temp_paths("bare");
        let source = _root.join("checkout");
        std::fs::create_dir_all(source.join("bin")).unwrap();
        std::fs::create_dir_all(source.join("glibc/lib")).unwrap();
        std::fs::write(source.join("bin/rosemary"), b"#!/bin/sh\n").unwrap();
        std::fs::write(source.join("glibc/lib/ld.so"), b"binary").unwrap();

        let id = import_custom_at(&paths, &source, "Bare").unwrap();

        let dir = paths.runners().join(&id);
        assert!(dir.join("Rosemary/glibc/lib/ld.so").is_file());
        assert!(dir.join("Rosemary/bin/rosemary").is_file());
        assert!(dir.join("bin/rosemary").is_file());
        let _ = std::fs::remove_dir_all(_root);
    }

    #[test]
    fn the_cached_tag_remembers_the_newest_release() {
        let (_root, paths) = temp_paths("cache");

        assert_eq!(cached_latest(&paths), None);
        remember_latest(&paths, "v0.3.2");
        assert_eq!(cached_latest(&paths), Some("v0.3.2".to_string()));

        // a torn or corrupted cache reads as absent, not fatal.
        std::fs::write(paths.rosemary_cache(), b"garbage").unwrap();
        assert_eq!(cached_latest(&paths), None);
        let _ = std::fs::remove_dir_all(_root);
    }

    #[test]
    fn a_down_api_serves_installed_runners() {
        let (_root, paths) = temp_paths("offline");
        assert_eq!(cached_id(&paths), None);

        let official = paths.runners().join(id_for("v0.3.0"));
        std::fs::create_dir_all(official.join("bin")).unwrap();
        std::fs::write(official.join("bin/rosemary"), b"#!/bin/sh\n").unwrap();
        assert_eq!(cached_id(&paths), Some(id_for("v0.3.0")));

        // a cache whose build is still installed wins over plain "newest".
        remember_latest(&paths, "v0.3.4");
        let newer = paths.runners().join(id_for("v0.3.4"));
        std::fs::create_dir_all(newer.join("bin")).unwrap();
        std::fs::write(newer.join("bin/rosemary"), b"#!/bin/sh\n").unwrap();
        assert_eq!(cached_id(&paths), Some(id_for("v0.3.4")));
        let _ = std::fs::remove_dir_all(_root);
    }

    #[test]
    fn a_taken_name_is_refused() {
        let (_root, paths) = temp_paths("taken");
        let source = _root.join("download");
        a_build(&source);

        import_custom_at(&paths, &source, "Mine").unwrap();
        let error = import_custom_at(&paths, &source, "Mine").unwrap_err();
        assert!(error.to_string().contains("already exists"));

        let official = paths.runners().join("rosemary-v0.3.0-official");
        std::fs::create_dir_all(official.join("bin")).unwrap();
        std::fs::write(official.join("bin/rosemary"), b"#!/bin/sh\n").unwrap();

        let error = import_custom_at(&paths, &source, "v0.3.0").unwrap_err();
        assert!(error.to_string().contains("already exists"));
        let error = import_custom_at(&paths, &source, "").unwrap_err();
        assert!(error.to_string().contains("cannot be empty"));
        let error = import_custom_at(&paths, &source, "sub/dir").unwrap_err();
        assert!(error.to_string().contains("plain folder name"));
        let _ = std::fs::remove_dir_all(_root);
    }

    #[test]
    fn shared_library_links_come_over_as_plain_files() {
        let (_root, paths) = temp_paths("links");
        let source = _root.join("download");
        a_build(&source);
        let lib = source.join("Rosemary/lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("libFLAC.so.8"), b"audio").unwrap();
        std::os::unix::fs::symlink("libFLAC.so.8", lib.join("libFLAC.so.14")).unwrap();
        let outside = source.join("Rosemary/somewhere");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("data"), b"tree").unwrap();
        std::os::unix::fs::symlink("../somewhere", lib.join("tree")).unwrap();

        let id = import_custom_at(&paths, &source, "Linked").unwrap();

        let dir = paths.runners().join(&id);
        assert_eq!(
            std::fs::read(dir.join("Rosemary/lib/libFLAC.so.14")).unwrap(),
            b"audio"
        );
        let copied = std::fs::read_to_string(dir.join("Rosemary/lib/tree/data")).unwrap();
        assert_eq!(copied, "tree");
        let _ = std::fs::remove_dir_all(_root);
    }

    #[test]
    fn a_failed_copy_leaves_the_name_free() {
        let (_root, paths) = temp_paths("partial");
        let source = _root.join("download");
        a_build(&source);
        std::os::unix::fs::symlink("../vanishing", source.join("Rosemary/gone")).unwrap();

        let error = import_custom_at(&paths, &source, "Meow").unwrap_err();
        assert!(error.to_string().contains("points at nothing"));

        assert!(!paths.runners().join("custom-Meow").exists());
        assert!(!name_taken_at(&paths, "Meow"));

        std::fs::remove_file(source.join("Rosemary/gone")).unwrap();
        let id = import_custom_at(&paths, &source, "Meow").unwrap();
        assert_eq!(id, "custom-Meow");
        let _ = std::fs::remove_dir_all(_root);
    }

    #[test]
    fn a_without_build_folder_is_refused() {
        let (_root, paths) = temp_paths("empty");
        let source = _root.join("nothing-here");
        std::fs::create_dir_all(&source).unwrap();

        let error = import_custom_at(&paths, &source, "Empty").unwrap_err();
        assert!(error.to_string().contains("no Rosemary build"));
        let _ = std::fs::remove_dir_all(_root);
    }
}
