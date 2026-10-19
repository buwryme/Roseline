// SPDX-License-Identifier: GPL-3.0-or-later

//! Roblox's own web APIs: a place's game, its name and its cover.

use anyhow::{Context, Result};

pub struct GameInfo {
    pub name: String,
    /// 768x432 cover, as a direct image URL.
    pub cover: String,
}

/// place → universe → name and cover, as the endpoints document.
pub fn resolve(place_id: u64) -> Result<GameInfo> {
    let universe: serde_json::Value = crate::net::get_json(
        &format!("https://apis.roblox.com/universes/v1/places/{place_id}/universe"),
        &[],
    )?;
    let universe_id = universe
        .get("universeId")
        .and_then(serde_json::Value::as_u64)
        .with_context(|| format!("place {place_id} has no universe"))?;

    let games: serde_json::Value = crate::net::get_json(
        &format!("https://games.roblox.com/v1/games?universeIds={universe_id}"),
        &[],
    )?;
    let name = games
        .pointer("/data/0/name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Roblox")
        .to_string();

    // the cover is decorative; a failure leaves it out.
    let cover = crate::net::get_json::<serde_json::Value>(
        &format!(
            "https://thumbnails.roblox.com/v1/games/multiget/thumbnails?universeIds={universe_id}&size=768x432&format=Png&isCircular=false"
        ),
        &[],
    )
    .ok()
    .and_then(|thumbs| {
        thumbs
            .pointer("/data/0/thumbnails/0/imageUrl")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    })
    .unwrap_or_default();

    Ok(GameInfo { name, cover })
}
