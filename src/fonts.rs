// SPDX-License-Identifier: GPL-3.0-or-later

//! font overrides for the client's bundled text fonts.
//!
//! one chosen file replaces every matching font in `content/fonts`, and the
//! family maps are rewritten so weights the client normally streams use the
//! same file too. language fallbacks, emoji fonts, and icon fonts stay alone.
//! the originals are kept per client version so a reset can put them back.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Paths;
use crate::settings::Settings;

/// where the client keeps its bundled text fonts.
pub const ROOT: &str = "Contents/Resources/content/fonts";
/// the family maps inside `ROOT`.
const FAMILIES: &str = "families";
/// marker for the client version the backups belong to.
const BACKUP_VERSION: &str = "version";

/// `Text` is the ordinary UI/game font choice; `Mono` is the code choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Mono,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub path: PathBuf,
    pub relative: PathBuf,
    pub kind: Kind,
    /// a `families/*.json` map rather than a font file.
    pub family: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Summary {
    pub written: usize,
    pub restored: usize,
}

/// the sfnt signatures a `.ttf`, `.otf`, or collection may start with.
const MAGICS: [&[u8; 4]; 5] = [b"\0\x01\0\0", b"OTTO", b"true", b"typ1", b"ttcf"];

fn is_font(data: &[u8]) -> bool {
    data.len() >= 4 && MAGICS.iter().any(|magic| data.starts_with(*magic))
}

fn read_font(path: &Path) -> Result<Vec<u8>> {
    let data = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    if !is_font(&data) {
        bail!("{} is not a TrueType or OpenType font", path.display());
    }
    Ok(data)
}

/// take a copy into Roseline's data directory, so a portal grant or a moved
/// download cannot leave the launch without a font next time.
pub fn import(source: &Path, kind: Kind) -> Result<PathBuf> {
    import_at(&Paths::get(), source, kind)
}

fn import_at(paths: &Paths, source: &Path, kind: Kind) -> Result<PathBuf> {
    let data = read_font(source)?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .context("the font file name is not valid UTF-8")?;
    let directory = paths.font_store().join(store_dir(kind));
    if directory.exists() {
        fs::remove_dir_all(&directory)?;
    }
    fs::create_dir_all(&directory)?;
    let destination = directory.join(name);
    crate::util::write_atomic(&destination, &data)?;
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o644))?;
    Ok(destination)
}

/// remove the imported copy for a choice; external paths are left alone.
pub fn discard(kind: Kind) {
    discard_at(&Paths::get(), kind);
}

fn discard_at(paths: &Paths, kind: Kind) {
    let _ = fs::remove_dir_all(paths.font_store().join(store_dir(kind)));
}

fn store_dir(kind: Kind) -> &'static str {
    match kind {
        Kind::Text => "text",
        Kind::Mono => "mono",
    }
}

/// the choice a bundled font file belongs to, or `None` for protected files.
pub fn classify(name: &str) -> Option<Kind> {
    let lower = name.to_ascii_lowercase();
    if !(lower.ends_with(".ttf") || lower.ends_with(".otf")) {
        return None;
    }
    // Roblox emoji fonts carry a private glyph table, not a normal emoji
    // cmap; replacing them with a text face breaks emoji instead of theming.
    if lower.contains("emoji") {
        return None;
    }
    // Noto files are script fallbacks: Thai, Arabic, Bengali, Devanagari,
    // Georgian, Khmer, Myanmar, Sinhala, and CJK.
    if lower.starts_with("noto") {
        return None;
    }
    // Inconsolata is Roblox's older `Code` face; it is monospaced even
    // though its name does not say so. Press Start 2P is fixed-width too,
    // but it is the Arcade display family, so it follows the text choice.
    if lower.contains("mono") || lower == "inconsolata-regular.ttf" {
        return Some(Kind::Mono);
    }
    Some(Kind::Text)
}

/// the choice a `families/*.json` map belongs to, or `None` to leave it.
fn classify_family(name: &str) -> Option<Kind> {
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".json") || lower.starts_with("noto") || lower.contains("emoji") {
        return None;
    }
    if lower.contains("mono") || lower == "inconsolata.json" {
        Some(Kind::Mono)
    } else {
        Some(Kind::Text)
    }
}

/// every bundled font and family map under the managed client copy.
pub fn targets(paths: &Paths) -> Vec<Target> {
    let root = paths.app_bundle().join(ROOT);
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut targets = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(kind) = classify(&name) else {
            continue;
        };
        push_target(paths, &mut targets, entry.path(), kind, false);
    }
    if let Ok(entries) = fs::read_dir(root.join(FAMILIES)) {
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(kind) = classify_family(&name) else {
                continue;
            };
            push_target(paths, &mut targets, entry.path(), kind, true);
        }
    }
    targets.sort_by(|left, right| left.relative.cmp(&right.relative));
    targets
}

fn push_target(paths: &Paths, targets: &mut Vec<Target>, path: PathBuf, kind: Kind, family: bool) {
    let Ok(relative) = path.strip_prefix(paths.app_bundle()).map(Path::to_path_buf) else {
        return;
    };
    targets.push(Target {
        path,
        relative,
        kind,
        family,
    });
}

/// how many actual font files (not family maps) one choice would replace.
pub fn count(paths: &Paths, kind: Kind) -> usize {
    targets(paths)
        .into_iter()
        .filter(|target| target.kind == kind && !target.family)
        .count()
}

/// apply the settings to the managed client copy.
pub fn apply(settings: &Settings) -> Result<Summary> {
    apply_at(&Paths::get(), settings)
}

pub fn apply_at(paths: &Paths, settings: &Settings) -> Result<Summary> {
    let targets = targets(paths);
    if targets.is_empty() {
        return Ok(Summary::default());
    }

    let text = read_choice(settings.fonts.font.as_deref(), "font");
    let mono = read_choice(settings.fonts.mono.as_deref(), "monospace font");
    let backup = paths.font_backup();
    let version = crate::roblox::installed_upload_at(&paths.app_version()).unwrap_or_default();
    let version_file = backup.join(BACKUP_VERSION);
    let stored = fs::read_to_string(&version_file).ok();
    let matching = stored.as_deref() == Some(version.as_str());

    if !matching {
        // a new client replaced the copy wholesale; old originals are stale.
        if backup.exists() {
            fs::remove_dir_all(&backup)?;
        }
        if text.is_none() && mono.is_none() {
            return Ok(Summary::default());
        }
        fs::create_dir_all(&backup)?;
        crate::util::write_atomic(&version_file, version.as_bytes())?;
    } else if text.is_none() && mono.is_none() && !backup.exists() {
        return Ok(Summary::default());
    }

    let text_reference = reference(&targets, Kind::Text);
    let mono_reference = reference(&targets, Kind::Mono);
    let mut summary = Summary::default();
    for target in &targets {
        let (chosen, reference) = match target.kind {
            Kind::Text => (text.as_deref(), text_reference.as_deref()),
            Kind::Mono => (mono.as_deref(), mono_reference.as_deref()),
        };
        match (target.family, chosen, reference) {
            (true, Some(_), Some(reference)) => {
                apply_family(target, &backup, reference, &mut summary)?;
            }
            (false, Some(bytes), _) => {
                apply_font(target, &backup, bytes, &mut summary)?;
            }
            _ => restore_one(target, &backup, &mut summary)?,
        }
    }
    prune_empty_dirs(&backup);
    Ok(summary)
}

/// the local font every family map of this choice points at.
fn reference(targets: &[Target], kind: Kind) -> Option<String> {
    let preferred = match kind {
        Kind::Text => "Roboto-Regular.ttf",
        Kind::Mono => "RobotoMono-Regular.ttf",
    };
    let local = || {
        targets
            .iter()
            .filter(move |target| target.kind == kind && !target.family)
    };
    if local().any(|target| {
        target
            .path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().as_ref() == preferred)
    }) {
        return Some(preferred.to_string());
    }
    local().find_map(|target| {
        target
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    })
}

/// read a chosen file once; a missing or broken file falls back to the
/// client's own fonts rather than blocking a launch.
fn read_choice(path: Option<&Path>, label: &str) -> Option<Vec<u8>> {
    let path = path?;
    match fs::read(path) {
        Ok(data) if is_font(&data) => Some(data),
        Ok(_) => {
            eprintln!(
                "roseline: the selected {label} {} is not a font; using the client's own files",
                path.display()
            );
            None
        }
        Err(error) => {
            eprintln!(
                "roseline: could not read the selected {label} {}: {error}; using the client's own files",
                path.display()
            );
            None
        }
    }
}

fn apply_font(target: &Target, backup: &Path, bytes: &[u8], summary: &mut Summary) -> Result<()> {
    let current = fs::read(&target.path)
        .with_context(|| format!("could not read {}", target.path.display()))?;
    if current == bytes {
        return Ok(());
    }
    backup_target(target, backup)?;
    crate::util::write_atomic(&target.path, bytes)
        .with_context(|| format!("could not replace {}", target.path.display()))?;
    fs::set_permissions(&target.path, fs::Permissions::from_mode(0o644))?;
    summary.written += 1;
    Ok(())
}

/// point every face in a family map at the chosen local file, including the
/// faces that normally download from Roblox's CDN.
fn apply_family(
    target: &Target,
    backup: &Path,
    reference: &str,
    summary: &mut Summary,
) -> Result<()> {
    let text = fs::read_to_string(&target.path)
        .with_context(|| format!("could not read {}", target.path.display()))?;
    let mut family: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("could not parse {}", target.path.display()))?;
    let asset = format!("rbxasset://fonts/{reference}");
    let Some(faces) = family
        .get_mut("faces")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(());
    };
    let mut changed = false;
    for face in faces {
        let Some(face) = face.as_object_mut() else {
            continue;
        };
        if face.get("assetId").and_then(serde_json::Value::as_str) != Some(asset.as_str()) {
            face.insert(
                "assetId".to_string(),
                serde_json::Value::String(asset.clone()),
            );
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    backup_target(target, backup)?;
    let mut output = serde_json::to_string_pretty(&family)?;
    output.push('\n');
    crate::util::write_atomic(&target.path, output.as_bytes())?;
    fs::set_permissions(&target.path, fs::Permissions::from_mode(0o644))?;
    summary.written += 1;
    Ok(())
}

fn backup_target(target: &Target, backup: &Path) -> Result<()> {
    let path = backup.join(&target.relative);
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&target.path, &path)
        .with_context(|| format!("could not back up {}", target.path.display()))?;
    Ok(())
}

fn restore_one(target: &Target, backup: &Path, summary: &mut Summary) -> Result<()> {
    let path = backup.join(&target.relative);
    let Ok(bytes) = fs::read(&path) else {
        return Ok(());
    };
    crate::util::write_atomic(&target.path, &bytes)
        .with_context(|| format!("could not restore {}", target.path.display()))?;
    fs::set_permissions(&target.path, fs::Permissions::from_mode(0o644))?;
    fs::remove_file(&path)?;
    summary.restored += 1;
    Ok(())
}

fn prune_empty_dirs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let path = entry.path();
            prune_empty_dirs(&path);
            let _ = fs::remove_dir(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> (PathBuf, Paths) {
        let root =
            std::env::temp_dir().join(format!("roseline-fonts-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            data: root.clone(),
            config: root.clone(),
            cache: root.clone(),
        };
        std::fs::create_dir_all(paths.app()).unwrap();
        std::fs::write(paths.app_version(), b"version-1").unwrap();
        (root, paths)
    }

    fn stock(paths: &Paths, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = paths.app_bundle().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn settings(root: &Path) -> Settings {
        let mut settings = Settings::default();
        let text = root.join("inter.ttf");
        let mono = root.join("jetbrains.ttf");
        std::fs::write(&text, b"\0\x01\0\0inter").unwrap();
        std::fs::write(&mono, b"\0\x01\0\0jetbrains").unwrap();
        settings.fonts.font = Some(text);
        settings.fonts.mono = Some(mono);
        settings
    }

    #[test]
    fn classifies_bundled_fonts() {
        assert_eq!(classify("BuilderSans-Regular.otf"), Some(Kind::Text));
        assert_eq!(classify("AccanthisADFStd-Regular.otf"), Some(Kind::Text));
        assert_eq!(classify("PressStart2P-Regular.ttf"), Some(Kind::Text));
        assert_eq!(classify("BuilderMono-Bold.otf"), Some(Kind::Mono));
        assert_eq!(classify("RobotoMono-Regular.ttf"), Some(Kind::Mono));
        assert_eq!(classify("Inconsolata-Regular.ttf"), Some(Kind::Mono));
        assert_eq!(classify("NotoSansThaiUI-Regular.ttf"), None);
        assert_eq!(classify("NotoNaskhArabicUI-Regular.ttf"), None);
        assert_eq!(classify("TwemojiMozilla.ttf"), None);
        assert_eq!(classify("RobloxEmoji.ttf"), None);
        assert_eq!(classify("gamecontrollerdb.txt"), None);

        assert_eq!(classify_family("BuilderMono.json"), Some(Kind::Mono));
        assert_eq!(classify_family("RobotoMono.json"), Some(Kind::Mono));
        assert_eq!(classify_family("Inconsolata.json"), Some(Kind::Mono));
        assert_eq!(classify_family("Roboto.json"), Some(Kind::Text));
        assert_eq!(classify_family("NotoSansCJKFallback.json"), None);
    }

    #[test]
    fn applies_and_restores_fonts_and_family_maps() {
        let (root, paths) = temp_paths("apply");
        let text = stock(
            &paths,
            "Contents/Resources/content/fonts/Roboto-Regular.ttf",
            b"stock-text",
        );
        let builder = stock(
            &paths,
            "Contents/Resources/content/fonts/BuilderSans-Regular.otf",
            b"stock-builder",
        );
        let mono = stock(
            &paths,
            "Contents/Resources/content/fonts/RobotoMono-Regular.ttf",
            b"stock-mono",
        );
        let thai = stock(
            &paths,
            "Contents/Resources/content/fonts/NotoSansThaiUI-Regular.ttf",
            b"stock-thai",
        );
        let emoji = stock(
            &paths,
            "Contents/Resources/content/fonts/TwemojiMozilla.ttf",
            b"stock-emoji",
        );
        let family = stock(
            &paths,
            "Contents/Resources/content/fonts/families/Roboto.json",
            br#"{
  "name": "Roboto",
  "faces": [
    {"name": "Regular", "weight": 400, "style": "normal", "assetId": "rbxasset://fonts/Roboto-Regular.ttf"},
    {"name": "Bold", "weight": 700, "style": "normal", "assetId": "rbxassetid://123"}
  ]
}
"#,
        );
        let mono_family = stock(
            &paths,
            "Contents/Resources/content/fonts/families/RobotoMono.json",
            br#"{"name":"Roboto Mono","faces":[{"name":"Regular","weight":400,"style":"normal","assetId":"rbxassetid://456"}]}"#,
        );
        let cjk_family = stock(
            &paths,
            "Contents/Resources/content/fonts/families/NotoSansCJKFallback.json",
            br#"{"name":"NotoSansCJK Fallback","faces":[{"assetId":"rbxassetid://789"}]}"#,
        );
        let settings = settings(&root);
        let text_bytes = std::fs::read(settings.fonts.font.as_ref().unwrap()).unwrap();
        let mono_bytes = std::fs::read(settings.fonts.mono.as_ref().unwrap()).unwrap();

        let applied = apply_at(&paths, &settings).unwrap();
        assert_eq!(applied.written, 5);
        assert_eq!(std::fs::read(&text).unwrap(), text_bytes);
        assert_eq!(std::fs::read(&builder).unwrap(), text_bytes);
        assert_eq!(std::fs::read(&mono).unwrap(), mono_bytes);
        assert_eq!(std::fs::read(&thai).unwrap(), b"stock-thai");
        assert_eq!(std::fs::read(&emoji).unwrap(), b"stock-emoji");

        let family_text = std::fs::read_to_string(&family).unwrap();
        let family_json: serde_json::Value = serde_json::from_str(&family_text).unwrap();
        for face in family_json["faces"].as_array().unwrap() {
            assert_eq!(face["assetId"], "rbxasset://fonts/Roboto-Regular.ttf");
        }
        let mono_text = std::fs::read_to_string(&mono_family).unwrap();
        let mono_json: serde_json::Value = serde_json::from_str(&mono_text).unwrap();
        assert_eq!(
            mono_json["faces"][0]["assetId"],
            "rbxasset://fonts/RobotoMono-Regular.ttf"
        );
        assert_eq!(
            std::fs::read_to_string(&cjk_family).unwrap(),
            r#"{"name":"NotoSansCJK Fallback","faces":[{"assetId":"rbxassetid://789"}]}"#
        );

        let mut settings = settings;
        settings.fonts.font = None;
        settings.fonts.mono = None;
        let restored = apply_at(&paths, &settings).unwrap();
        assert_eq!(restored.restored, 5);
        assert_eq!(std::fs::read(&text).unwrap(), b"stock-text");
        assert_eq!(std::fs::read(&builder).unwrap(), b"stock-builder");
        assert_eq!(std::fs::read(&mono).unwrap(), b"stock-mono");
        assert!(
            std::fs::read_to_string(&family)
                .unwrap()
                .contains("rbxassetid://123")
        );
        assert!(
            std::fs::read_to_string(&mono_family)
                .unwrap()
                .contains("rbxassetid://456")
        );
        assert!(
            !paths
                .font_backup()
                .join("Contents/Resources/content/fonts/families/Roboto.json")
                .exists()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_choice_falls_back_to_the_original() {
        let (root, paths) = temp_paths("missing");
        let target = stock(
            &paths,
            "Contents/Resources/content/fonts/Roboto-Regular.ttf",
            b"stock",
        );
        let mut settings = settings(&root);
        apply_at(&paths, &settings).unwrap();
        settings.fonts.font = Some(root.join("gone.ttf"));
        let summary = apply_at(&paths, &settings).unwrap();
        assert_eq!(summary.restored, 1);
        assert_eq!(std::fs::read(&target).unwrap(), b"stock");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn imports_a_stable_copy_and_discards_it() {
        let (root, paths) = temp_paths("import");
        let source = root.join("Inter.ttf");
        std::fs::write(&source, b"\0\x01\0\0inter").unwrap();
        let imported = import_at(&paths, &source, Kind::Text).unwrap();
        assert!(imported.starts_with(paths.font_store()));
        assert_eq!(std::fs::read(&imported).unwrap(), b"\0\x01\0\0inter");
        assert_eq!(imported.file_name().unwrap(), "Inter.ttf");
        discard_at(&paths, Kind::Text);
        assert!(!imported.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_non_font_data() {
        let (root, _) = temp_paths("validate");
        let font = root.join("font.ttf");
        std::fs::write(&font, b"\0\x01\0\0rest").unwrap();
        assert!(read_font(&font).is_ok());
        std::fs::write(&font, b"not a font").unwrap();
        assert!(read_font(&font).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
