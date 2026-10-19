// SPDX-License-Identifier: GPL-3.0-or-later

//! fast flags, locked to Roblox's allowlist.
//!
//! the client ignores everything else, so there is no free-form editor.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::config::Paths;
use crate::settings::Settings;

pub const ALLOWLIST_URL: &str =
    "https://devforum.roblox.com/t/allowlist-for-local-client-configuration-via-fast-flags/3966569";

pub struct Choice {
    pub label: &'static str,
    /// `None` is "Default": the key is left out.
    pub value: Option<&'static str>,
}

pub struct Preset {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub choices: &'static [Choice],
}

pub const PRESETS: [Preset; 3] = [
    Preset {
        key: "FIntDebugForceMSAASamples",
        label: "MSAA samples",
        description: "Antialiasing sample count.",
        icon: "video-display-symbolic",
        choices: &[
            Choice {
                label: "Default",
                value: None,
            },
            Choice {
                label: "Off",
                value: Some("0"),
            },
            Choice {
                label: "2×",
                value: Some("2"),
            },
            Choice {
                label: "4×",
                value: Some("4"),
            },
            Choice {
                label: "8×",
                value: Some("8"),
            },
        ],
    },
    Preset {
        key: "FIntGrassMovementReducedMotionFactor",
        label: "Grass motion",
        description: "How much grass sways, if the renderer bothers at all.",
        icon: "weather-windy-symbolic",
        choices: &[
            Choice {
                label: "Default",
                value: None,
            },
            Choice {
                label: "Off",
                value: Some("0"),
            },
        ],
    },
    Preset {
        key: "FFlagDebugSkyGray",
        label: "Grey sky",
        description: "Replaces the sky with flat grey, as a rendering debug aid.",
        icon: "weather-clear-symbolic",
        choices: &[
            Choice {
                label: "Default",
                value: None,
            },
            Choice {
                label: "On",
                value: Some("True"),
            },
        ],
    },
];

/// the allowlisted selections: presets and override sliders.
pub fn payload(settings: &Settings) -> BTreeMap<String, String> {
    settings
        .flags
        .iter()
        .filter(|(key, _)| {
            PRESETS.iter().any(|preset| preset.key == key.as_str())
                || EXTRA_KEYS.contains(&key.as_str())
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// keys the override sliders write directly.
pub const EXTRA_KEYS: [&str; 6] = [
    "DFIntCSGLevelOfDetailSwitchingDistance",
    "DFIntCSGLevelOfDetailSwitchingDistanceL12",
    "DFIntCSGLevelOfDetailSwitchingDistanceL23",
    "DFIntCSGLevelOfDetailSwitchingDistanceL34",
    "DFIntTextureQualityOverride",
    "DFFlagTextureQualityOverrideEnabled",
];

/// write `ClientAppSettings.json` into the managed copy.
pub fn write(settings: &Settings) -> Result<PathBuf> {
    let directory = Paths::get()
        .app_bundle()
        .join("Contents/MacOS/ClientSettings");
    let path = directory.join("ClientAppSettings.json");
    let text = serde_json::to_string_pretty(&payload(settings))?;
    crate::util::write_atomic(&path, text.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_keeps_allowlisted_selections_only() {
        let mut settings = Settings::default();
        settings
            .flags
            .insert("FIntDebugForceMSAASamples".into(), "4".into());
        settings
            .flags
            .insert("FFlagNotAllowlisted".into(), "True".into());
        let payload = payload(&settings);
        assert_eq!(payload.len(), 1);
        assert_eq!(
            payload.get("FIntDebugForceMSAASamples").map(String::as_str),
            Some("4")
        );
    }

    #[test]
    fn preset_keys_are_unique() {
        let mut keys: Vec<&str> = PRESETS.iter().map(|preset| preset.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), PRESETS.len());
        for preset in &PRESETS {
            assert!(preset.choices.iter().any(|choice| choice.value.is_none()));
        }
    }
}
