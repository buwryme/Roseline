// SPDX-License-Identifier: GPL-3.0-or-later

//! content overrides, applied onto the managed copy.
//!
//! the overlay mirrors the bundle; a state file skips unchanged files and
//! re-applies everything after a client update.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Paths;
use crate::progress::Progress;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    app_version: String,
    files: BTreeMap<String, Record>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    src_size: u64,
    src_mtime: i64,
    dst_size: u64,
    dst_mtime: i64,
}

/// apply the overlay; returns how many files were written.
pub fn apply(progress: &Progress) -> Result<usize> {
    let paths = Paths::get();
    let overlay = paths.overlay();
    if !overlay.is_dir() {
        return Ok(0);
    }
    let version = crate::roblox::installed_upload().unwrap_or_default();
    let mut state: State = std::fs::read_to_string(paths.overlay_state())
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    // a different client version means the copy was replaced wholesale.
    if state.app_version != version {
        state = State {
            app_version: version,
            files: BTreeMap::new(),
        };
    }

    let mut sources = Vec::new();
    collect(&overlay, &overlay, &mut sources)?;
    if sources.is_empty() {
        return Ok(0);
    }
    progress.stage("Applying the content overlay");

    let mut copied = 0;
    for (relative, source) in &sources {
        let destination = paths.app_bundle().join(relative);
        let source_meta = std::fs::metadata(source)?;
        if let Some(stored) = state.files.get(relative) {
            if stored.src_size == source_meta.len() && stored.src_mtime == mtime(&source_meta) {
                if let Ok(dst_meta) = std::fs::metadata(&destination) {
                    if stored.dst_size == dst_meta.len() && stored.dst_mtime == mtime(&dst_meta) {
                        continue;
                    }
                }
            }
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source, &destination)?;
        std::fs::set_permissions(&destination, source_meta.permissions())?;
        let dst_meta = std::fs::metadata(&destination)?;
        state.files.insert(
            relative.clone(),
            Record {
                src_size: source_meta.len(),
                src_mtime: mtime(&source_meta),
                dst_size: dst_meta.len(),
                dst_mtime: mtime(&dst_meta),
            },
        );
        copied += 1;
    }

    // removed files stop being tracked; what they wrote stays until an update.
    let seen: Vec<String> = sources
        .iter()
        .map(|(relative, _)| relative.clone())
        .collect();
    state.files.retain(|key, _| seen.contains(key));

    crate::util::write_atomic(
        &paths.overlay_state(),
        toml::to_string_pretty(&state)?.as_bytes(),
    )?;
    Ok(copied)
}

fn mtime(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_nanos() as i64)
        .unwrap_or(0)
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect(root, &path, out)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root)?.to_string_lossy().into_owned();
            out.push((relative, path));
        }
    }
    Ok(())
}

/// copy the stock files for `relative` into the overlay, skipping subtrees
/// a sibling category owns; returns (copied, skipped).
pub fn seed_excluding(relative: &str, skip: &[&str]) -> Result<(usize, usize)> {
    let source = Paths::get().app_bundle().join(relative);
    let target = Paths::get().overlay().join(relative);
    if !source.is_dir() {
        return Ok((0, 0));
    }
    seed_dir(&source, &target, &source, skip)
}

fn seed_dir(source: &Path, target: &Path, root: &Path, skip: &[&str]) -> Result<(usize, usize)> {
    let mut copied = 0;
    let mut skipped = 0;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if let Ok(relative) = from.strip_prefix(root)
                && skip.iter().any(|name| relative == Path::new(name))
            {
                continue;
            }
            let (inner_copied, inner_skipped) = seed_dir(&from, &to, root, skip)?;
            copied += inner_copied;
            skipped += inner_skipped;
        } else if to.exists() {
            skipped += 1;
        } else {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&from, &to)?;
            copied += 1;
        }
    }
    Ok((copied, skipped))
}

/// files and bytes the overlay holds for `relative`.
pub fn measure(relative: &str) -> (usize, u64) {
    let mut files = 0;
    let mut bytes = 0;
    walk(&Paths::get().overlay().join(relative), &mut |meta| {
        files += 1;
        bytes += meta.len();
    });
    (files, bytes)
}

/// remove everything the overlay holds for `relative`.
pub fn clear(relative: &str) -> Result<()> {
    let path = Paths::get().overlay().join(relative);
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn walk(dir: &Path, visit: &mut impl FnMut(&std::fs::Metadata)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(&entry.path(), visit);
        } else if let Ok(meta) = entry.metadata() {
            visit(&meta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_nested_directories() {
        let root = std::env::temp_dir().join(format!("roseline-overlay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/deep.ttf"), b"x").unwrap();
        std::fs::write(root.join("top.png"), b"y").unwrap();
        let mut sources = Vec::new();
        collect(&root, &root, &mut sources).unwrap();
        sources.sort();
        let names: Vec<String> = sources
            .iter()
            .map(|(relative, _)| relative.clone())
            .collect();
        assert_eq!(names, ["a/b/deep.ttf", "top.png"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn seeding_never_overwrites() {
        let root = std::env::temp_dir().join(format!("roseline-seed-{}", std::process::id()));
        let source = root.join("from");
        let target = root.join("to");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::write(source.join("a.ttf"), b"stock").unwrap();
        std::fs::write(source.join("nested/b.ttf"), b"stock").unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.ttf"), b"mine").unwrap();

        let (copied, skipped) = seed_dir(&source, &target, &source, &[]).unwrap();
        assert_eq!((copied, skipped), (1, 1));
        assert_eq!(std::fs::read(target.join("a.ttf")).unwrap(), b"mine");
        assert_eq!(
            std::fs::read(target.join("nested/b.ttf")).unwrap(),
            b"stock"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn skipping_hands_a_subtree_to_its_own_category() {
        let root = std::env::temp_dir().join(format!("roseline-skip-{}", std::process::id()));
        let source = root.join("from");
        let target = root.join("to");
        std::fs::create_dir_all(source.join("sky")).unwrap();
        std::fs::create_dir_all(source.join("other")).unwrap();
        std::fs::write(source.join("sky/face.tex"), b"1").unwrap();
        std::fs::write(source.join("other/plain.tex"), b"2").unwrap();

        let (copied, skipped) = seed_dir(&source, &target, &source, &["sky"]).unwrap();
        assert_eq!((copied, skipped), (1, 0));
        assert!(!target.join("sky").exists());
        assert!(target.join("other/plain.tex").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn state_round_trips() {
        let mut state = State {
            app_version: "version-abc".into(),
            files: BTreeMap::new(),
        };
        state.files.insert(
            "Contents/Resources/content/fonts/X.ttf".into(),
            Record {
                src_size: 1,
                src_mtime: 2,
                dst_size: 1,
                dst_mtime: 3,
            },
        );
        let text = toml::to_string_pretty(&state).unwrap();
        let parsed: State = toml::from_str(&text).unwrap();
        assert_eq!(parsed.app_version, "version-abc");
        assert!(
            parsed
                .files
                .contains_key("Contents/Resources/content/fonts/X.ttf")
        );
    }
}
