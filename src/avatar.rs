// SPDX-License-Identifier: GPL-3.0-or-later

//! the signed-in user's avatar head, for the Discord small image.

use std::sync::Mutex;

use serde_json::Value;

struct Headshot {
    ready: bool,
    url: Option<String>,
}

static HEADSHOT: Mutex<Headshot> = Mutex::new(Headshot {
    ready: false,
    url: None,
});

pub fn ready() -> bool {
    HEADSHOT.lock().map(|guard| guard.ready).unwrap_or(false)
}

pub fn cached() -> Option<String> {
    HEADSHOT
        .lock()
        .ok()
        .filter(|guard| guard.ready)
        .and_then(|guard| guard.url.clone())
}

pub fn headshot() -> Option<String> {
    let url = fetch();
    if let Ok(mut guard) = HEADSHOT.lock() {
        guard.ready = true;
        guard.url = url.clone();
    }
    url
}

fn fetch() -> Option<String> {
    let session = crate::cookies::session()?;
    let header = format!(".ROBLOSECURITY={session}");
    let headers = [("Cookie", header.as_str())];
    let user: Value =
        crate::net::get_json("https://users.roblox.com/v1/users/authenticated", &headers).ok()?;
    let id = user.get("id")?.as_u64()?;
    let thumbs: Value = crate::net::get_json(&headshot_url(id), &[]).ok()?;
    thumbs
        .pointer("/data/0/imageUrl")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|url| !url.is_empty())
}

fn headshot_url(id: u64) -> String {
    format!(
        "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={id}&size=150x150&format=Png&isCircular=true"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_headshot_url_points_at_the_users_endpoint() {
        assert_eq!(
            headshot_url(123),
            "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds=123&size=150x150&format=Png&isCircular=true"
        );
    }
}
