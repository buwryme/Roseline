// SPDX-License-Identifier: GPL-3.0-or-later

//! user preferences, stored as TOML so source builds need no schema install.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::Paths;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub graphics: Graphics,
    pub compatibility: Compatibility,
    pub prefix: Prefix,
    #[serde(default, skip_serializing_if = "Fonts::is_empty")]
    pub fonts: Fonts,
    /// pinned runner id; the newest installed when unset.
    pub runner: Option<String>,
    /// flag selections by name; only allowlisted presets are used.
    pub flags: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub user_flags: BTreeMap<String, String>,
    pub discord: Discord,
    pub server_location: ServerLocation,
    pub behavior: Behavior,
    pub environment: Environment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Compatibility {
    /// Allow x86_64 runners through muvm on ARM64 hosts.
    pub muvm: bool,
    pub vm_memory: VmMemory,
}

impl Default for Compatibility {
    fn default() -> Self {
        Self {
            muvm: cfg!(target_arch = "aarch64"),
            vm_memory: VmMemory::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VmMemory {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "4-gib")]
    FourGiB,
    #[serde(rename = "6-gib")]
    SixGiB,
    #[serde(rename = "8-gib")]
    EightGiB,
}

impl VmMemory {
    pub const ALL: [Self; 4] = [Self::Auto, Self::FourGiB, Self::SixGiB, Self::EightGiB];

    pub fn mib(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::FourGiB => Some(4096),
            Self::SixGiB => Some(6144),
            Self::EightGiB => Some(8192),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "muvm default",
            Self::FourGiB => "4 GiB",
            Self::SixGiB => "6 GiB",
            Self::EightGiB => "8 GiB",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Graphics {
    pub backend: Backend,
    /// PCI slot to render on; `None` is automatic.
    pub device: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    /// pick at launch: Vulkan when the host offers it, OpenGL otherwise.
    #[default]
    Auto,
    /// Metal through Vulkan, the fast path.
    Vulkan,
    /// no Metal at all: hide every Vulkan driver, use Rosemary's OpenGL.
    #[serde(rename = "opengl")]
    OpenGL,
}

impl Backend {
    pub const ALL: [Backend; 3] = [Backend::Auto, Backend::Vulkan, Backend::OpenGL];

    pub fn label(self) -> &'static str {
        match self {
            Backend::Auto => "Auto",
            Backend::Vulkan => "Vulkan",
            Backend::OpenGL => "OpenGL",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefix {
    pub system_files: SystemFiles,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Fonts {
    /// one font file copied over every bundled text font.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<PathBuf>,
    /// one font file copied over every bundled monospace font.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mono: Option<PathBuf>,
}

impl Fonts {
    fn is_empty(&self) -> bool {
        self.font.is_none() && self.mono.is_none()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemFiles {
    /// restore overwrites at the next launch.
    #[default]
    Repair,
    /// allow the overwrite and report it.
    Warn,
    /// refuse writes with EROFS.
    Immutable,
}

impl SystemFiles {
    pub const ALL: [SystemFiles; 3] = [
        SystemFiles::Repair,
        SystemFiles::Warn,
        SystemFiles::Immutable,
    ];

    pub fn value(self) -> &'static str {
        match self {
            SystemFiles::Repair => "repair",
            SystemFiles::Warn => "warn",
            SystemFiles::Immutable => "immutable",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SystemFiles::Repair => "Recover on launch",
            SystemFiles::Warn => "Warn only",
            SystemFiles::Immutable => "Read-only",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SystemFiles::Repair => "Overwrites are restored at the next start.",
            SystemFiles::Warn => "Overwrites are allowed, and reported.",
            SystemFiles::Immutable => "Writes to the base get EROFS.",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            SystemFiles::Repair => "changes-allow-symbolic",
            SystemFiles::Warn => "dialog-warning-symbolic",
            SystemFiles::Immutable => "changes-prevent-symbolic",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Environment {
    /// lines for the Mac programs, as `ROSEMARY_ENVIRON` takes them.
    pub guest: Vec<String>,
    /// `NAME=VALUE` lines for Rosemary; a bare `NAME` copies this computer's value.
    pub runner: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerLocation {
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Behavior {
    pub close_on_leave: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Discord {
    pub enabled: bool,
    /// whether the game itself is shown; off leaves the idle Roblox presence.
    pub show_game: bool,
    /// Discord application id; Rich Presence stays off while empty.
    pub app_id: String,
    /// a Join Game button that opens the server the user is on.
    pub join_button: bool,
    /// the avatar head replaces the Roblox small image while a game shows.
    pub show_avatar: bool,
    pub details: String,
    pub state: String,
}

/// shipped application id, replaceable and working out of the box.
pub const DEFAULT_APP_ID: &str = "1561415907023265872";

/// every config file starts with this; the serializer cannot carry comments.
const HEADER: &str =
    "# EDIT CAREFULLY! config corruption may cause Roseline to fail during launch.\n\n";

impl Default for Discord {
    fn default() -> Discord {
        Discord {
            enabled: false,
            show_game: true,
            app_id: DEFAULT_APP_ID.to_string(),
            join_button: false,
            show_avatar: false,
            details: "Roblox".into(),
            state: "via Roséline".into(),
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        let path = Paths::get().settings_file();
        let existed = path.is_file();
        let mut settings: Settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        // an empty id from an older version takes the shipped default.
        if settings.discord.app_id.trim().is_empty() {
            settings.discord.app_id = DEFAULT_APP_ID.to_string();
            let _ = settings.save();
        } else if !existed {
            // seed the editable template on first launch.
            let _ = settings.save();
        }
        settings
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Paths::get().settings_file();
        crate::util::write_atomic(&path, self.render()?.as_bytes())
    }

    fn render(&self) -> anyhow::Result<String> {
        let mut text = String::from(HEADER);
        text.push_str(&toml::to_string_pretty(self)?);
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_take_defaults() {
        let settings: Settings = toml::from_str("graphics = {}").unwrap();
        assert_eq!(settings.graphics.backend, Backend::Auto);
        assert!(settings.graphics.device.is_none());
        assert_eq!(settings.prefix.system_files, SystemFiles::Repair);
        assert!(settings.fonts.font.is_none());
        assert!(settings.fonts.mono.is_none());
        assert!(settings.runner.is_none());
        assert!(!settings.discord.enabled);
        assert!(!settings.server_location.enabled);
        assert!(settings.discord.show_game);
        assert!(!settings.discord.join_button);
        assert!(!settings.discord.show_avatar);
        assert!(settings.flags.is_empty());
        assert!(settings.user_flags.is_empty());
        assert!(settings.environment.guest.is_empty());
        assert!(settings.environment.runner.is_empty());
    }

    #[test]
    fn server_location_is_off_by_default_and_round_trips() {
        let defaults: Settings = toml::from_str("").unwrap();
        assert!(!defaults.server_location.enabled);
        let settings: Settings = toml::from_str("[server_location]\nenabled = true\n").unwrap();
        assert!(settings.server_location.enabled);
        let restored: Settings = toml::from_str(&settings.render().unwrap()).unwrap();
        assert!(restored.server_location.enabled);
    }

    #[test]
    fn manual_flags_are_read_from_the_config() {
        let settings: Settings =
            toml::from_str("[user_flags]\nFFlagSomething = \"True\"\n").unwrap();
        assert_eq!(
            settings
                .user_flags
                .get("FFlagSomething")
                .map(String::as_str),
            Some("True")
        );
    }

    #[test]
    fn close_on_leave_is_off_by_default_and_round_trips() {
        let defaults: Settings = toml::from_str("").unwrap();
        assert!(!defaults.behavior.close_on_leave);
        let settings: Settings = toml::from_str("[behavior]\nclose_on_leave = true\n").unwrap();
        assert!(settings.behavior.close_on_leave);
        let restored: Settings = toml::from_str(&settings.render().unwrap()).unwrap();
        assert!(restored.behavior.close_on_leave);
    }

    #[test]
    fn discord_ships_with_a_working_app_id() {
        assert_eq!(Discord::default().app_id, DEFAULT_APP_ID);
        assert!(!DEFAULT_APP_ID.trim().is_empty());
    }

    #[test]
    fn config_template_warns_at_the_top() {
        let text = Settings::default().render().unwrap();
        assert!(text.starts_with(
            "# EDIT CAREFULLY! config corruption may cause Roseline to fail during launch.\n\n"
        ));
        let parsed: Settings = toml::from_str(&text).unwrap();
        assert_eq!(parsed.discord.app_id, DEFAULT_APP_ID);
    }

    #[test]
    fn backend_round_trips() {
        let settings: Settings = toml::from_str("graphics = { backend = \"opengl\" }").unwrap();
        assert_eq!(settings.graphics.backend, Backend::OpenGL);
        let text = toml::to_string(&Settings::default()).unwrap();
        assert!(text.contains("backend = \"auto\""));
    }

    #[test]
    fn arm_compatibility_defaults_and_opt_out_survive_loading() {
        let defaults: Settings = toml::from_str("").unwrap();
        assert_eq!(defaults.compatibility.muvm, cfg!(target_arch = "aarch64"));
        let settings: Settings =
            toml::from_str("[compatibility]\nmuvm = false\nvm_memory = \"auto\"\n").unwrap();
        let restored: Settings = toml::from_str(&settings.render().unwrap()).unwrap();
        assert!(!restored.compatibility.muvm);
        assert_eq!(restored.compatibility.vm_memory, VmMemory::Auto);
        let capped: Settings = toml::from_str("[compatibility]\nvm_memory = \"4-gib\"\n").unwrap();
        assert_eq!(capped.compatibility.vm_memory.mib(), Some(4096));
    }

    #[test]
    fn system_files_names_are_the_rosemary_cli_words() {
        assert_eq!(SystemFiles::Repair.value(), "repair");
        assert_eq!(SystemFiles::Warn.value(), "warn");
        assert_eq!(SystemFiles::Immutable.value(), "immutable");
    }
}
