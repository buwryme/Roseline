// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP and hashing, the calls Butter proved against Rosemary's releases.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::progress::Progress;
use crate::util;

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(crate::config::user_agent())
        .timeout_connect(Some(Duration::from_secs(20)))
        .build()
        .into()
}

pub fn get_text(url: &str, headers: &[(&str, &str)]) -> Result<String> {
    let mut request = agent().get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request
        .call()
        .with_context(|| format!("could not reach {url}"))?;
    Ok(response.body_mut().read_to_string()?)
}

pub fn get_json<T: serde::de::DeserializeOwned>(url: &str, headers: &[(&str, &str)]) -> Result<T> {
    let text = get_text(url, headers)?;
    serde_json::from_str(&text).with_context(|| format!("{url} returned something unexpected"))
}

/// download `url` to `dest` through a `.part` file, so a killed download
/// never looks finished.
pub fn download(url: &str, dest: &Path, label: &str, progress: &Progress) -> Result<()> {
    let mut response = agent()
        .get(url)
        .call()
        .with_context(|| format!("could not download {url}"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = dest.with_extension("part");
    let mut file = std::fs::File::create(&partial)?;
    let mut reader = response.body_mut().with_config().limit(4 << 30).reader();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    let mut reported: u64 = 0;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buffer[..read])?;
        done += read as u64;
        if let Some(total) = total.filter(|total| *total > 0) {
            progress.progress(done as f64 / total as f64);
        }
        if done - reported >= 4 << 20 {
            progress.stage(match total {
                Some(total) => format!(
                    "{label} — {} of {}",
                    util::format_size(done),
                    util::format_size(total)
                ),
                None => format!("{label} — {}", util::format_size(done)),
            });
            reported = done;
        }
    }
    file.sync_all()?;
    std::fs::rename(&partial, dest)?;
    Ok(())
}

/// a small binary body, such as a game cover.
pub fn get_bytes(url: &str) -> Result<Vec<u8>> {
    let mut response = agent()
        .get(url)
        .call()
        .with_context(|| format!("could not reach {url}"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .with_config()
        .limit(16 << 20)
        .reader()
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// SHA-256 of a file, as lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
