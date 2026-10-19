// SPDX-License-Identifier: GPL-3.0-or-later

//! Rosemary's GitHub releases and the asset selection Butter proved.

use anyhow::{Context, Result};

use crate::config::{ROSEMARY_OWNER, ROSEMARY_REPO};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub digest: Option<String>,
}

impl Release {
    /// release name, or its tag.
    pub fn title(&self) -> &str {
        match self.name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => name,
            _ => &self.tag_name,
        }
    }

    /// the Steam tool archive for this architecture; `.tar.zst` first.
    pub fn steam_tool(&self) -> Option<&Asset> {
        let arch = std::env::consts::ARCH;
        ["tar.zst", "tar.gz"].iter().find_map(|extension| {
            let suffix = format!("-{arch}.{extension}");
            self.assets.iter().find(|asset| {
                asset.name.starts_with("rosemary-steam-tool-") && asset.name.ends_with(&suffix)
            })
        })
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

impl Asset {
    /// SHA-256 GitHub computed, when it states one.
    pub fn sha256(&self) -> Option<String> {
        let hex = self.digest.as_deref()?.strip_prefix("sha256:")?;
        (hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| hex.to_ascii_lowercase())
    }
}

/// the latest stable Rosemary release.
pub fn latest_release() -> Result<Release> {
    let url =
        format!("https://api.github.com/repos/{ROSEMARY_OWNER}/{ROSEMARY_REPO}/releases/latest");
    crate::net::get_json(
        &url,
        &[
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", "2022-11-28"),
        ],
    )
    .context("could not read Rosemary's latest release")
}

/// stable releases, newest first, for the version picker.
pub fn releases() -> Result<Vec<Release>> {
    let url = format!(
        "https://api.github.com/repos/{ROSEMARY_OWNER}/{ROSEMARY_REPO}/releases?per_page=30"
    );
    let list: Vec<Release> = crate::net::get_json(
        &url,
        &[
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", "2022-11-28"),
        ],
    )
    .context("could not list Rosemary's releases")?;
    Ok(list
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .collect())
}

/// checksum `sha256sum` lists for `name`, as lowercase hex.
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        // `sha256sum` marks binary-mode lines with `*`.
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.into(),
            browser_download_url: format!("https://example.invalid/{name}"),
            digest: None,
        }
    }

    #[test]
    fn picks_the_smallest_steam_tool() {
        let release = Release {
            tag_name: "v9.9.9".into(),
            name: None,
            draft: false,
            prerelease: false,
            assets: vec![
                asset("rosemary-steam-tool-9.9.9-aarch64.tar.zst"),
                asset("rosemary-steam-tool-9.9.9-x86_64.tar.gz"),
                asset("rosemary-steam-tool-9.9.9-x86_64.tar.zst"),
                asset("SHA256SUMS"),
            ],
        };
        let tool = release.steam_tool().unwrap();
        assert_eq!(tool.name, "rosemary-steam-tool-9.9.9-x86_64.tar.zst");
    }

    #[test]
    fn prefers_zstd_but_takes_gzip() {
        let release = Release {
            tag_name: "v9.9.9".into(),
            name: None,
            draft: false,
            prerelease: false,
            assets: vec![asset("rosemary-steam-tool-9.9.9-x86_64.tar.gz")],
        };
        assert_eq!(
            release.steam_tool().unwrap().name,
            "rosemary-steam-tool-9.9.9-x86_64.tar.gz"
        );
    }

    #[test]
    fn checksum_lines_are_read() {
        let sums = "\
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  other.tar.zst
abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd *rosemary-steam-tool-9.9.9-x86_64.tar.zst
";
        assert_eq!(
            checksum_for(sums, "rosemary-steam-tool-9.9.9-x86_64.tar.zst").as_deref(),
            Some("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd")
        );
        assert_eq!(checksum_for(sums, "missing"), None);
    }

    #[test]
    fn digest_parses_only_wellformed() {
        let mut tool = asset("x");
        tool.digest =
            Some("sha256:ABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD".into());
        assert!(tool.sha256().is_some());
        tool.digest = Some("sha256:short".into());
        assert!(tool.sha256().is_none());
    }
}
