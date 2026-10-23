// SPDX-License-Identifier: GPL-3.0-or-later

//! Discord Rich Presence over the local IPC socket.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub struct Discord {
    stream: UnixStream,
}

/// the name the presence shows; bridges read it from the payload, so it is
/// not cosmetic.
pub const NAME: &str = "ROBLOX";

/// Roblox's app icon on Wikimedia Commons; Discord fetches external images
/// itself, so a local `.icns` cannot be used.
pub const ROBLOX_LOGO: &str = "https://upload.wikimedia.org/wikipedia/commons/thumb/f/f2/Roblox_%282025%29_%28App_Icon%29.svg/960px-Roblox_%282025%29_%28App_Icon%29.svg.png";

/// one of the two links Discord shows under the presence.
pub struct Button {
    pub label: &'static str,
    pub url: String,
}

/// what the profile shows; images are plain URLs, which is how the game's
/// own cover gets there.
pub struct Activity {
    pub details: String,
    pub state: String,
    /// unix seconds the session started, for the elapsed timer.
    pub started: u64,
    pub large_image: Option<String>,
    pub small_image: Option<String>,
    /// deep link or game page opened by the presence button.
    pub button: Option<Button>,
}

/// connect, handshake and set the activity; `None` when Discord is absent,
/// which is fine.
pub fn start(app_id: &str, pid: u32, activity: &Activity) -> Option<Discord> {
    let mut stream = connect()?;
    let handshake = serde_json::json!({ "v": 1, "client_id": app_id });
    stream.write_all(&frame(0, &handshake)).ok()?;
    stream.flush().ok()?;
    let mut discord = Discord { stream };
    discord.set(pid, activity).then_some(discord)
}

impl Discord {
    pub fn set(&mut self, pid: u32, activity: &Activity) -> bool {
        let mut shown = serde_json::json!({
            "name": NAME,
            "details": activity.details,
            "state": activity.state,
            "timestamps": { "start": activity.started },
        });
        let mut assets = serde_json::Map::new();
        if let Some(image) = &activity.large_image {
            assets.insert(
                "large_image".to_string(),
                serde_json::Value::String(image.clone()),
            );
        }
        if let Some(image) = &activity.small_image {
            assets.insert(
                "small_image".to_string(),
                serde_json::Value::String(image.clone()),
            );
        }
        if !assets.is_empty() {
            shown["assets"] = serde_json::Value::Object(assets);
        }
        if let Some(button) = &activity.button {
            shown["buttons"] = serde_json::json!([{ "label": button.label, "url": button.url }]);
        }
        let payload = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": pid, "activity": shown },
            "nonce": format!("roseline-{pid}-{}", activity.started),
        });
        self.send(&payload)
    }

    /// take the activity down when the game exits.
    pub fn clear(&mut self, pid: u32) {
        let payload = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": pid, "activity": serde_json::Value::Null },
            "nonce": format!("roseline-clear-{pid}"),
        });
        self.send(&payload);
    }

    fn send(&mut self, payload: &serde_json::Value) -> bool {
        self.stream.write_all(&frame(1, payload)).is_ok() && self.stream.flush().is_ok()
    }
}

/// Whether this process runs inside a Flatpak sandbox.
pub fn sandboxed() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// Whether any Discord socket answers right now.
pub fn socket_available() -> bool {
    connect().is_some()
}

/// Discord's socket, under the names it and the usual wrappers use.
fn connect() -> Option<UnixStream> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    for index in 0..10 {
        for directory in [
            "",
            "snap.discord",
            "app/com.discordapp.Discord",
            "vesktop",
            "discord",
        ] {
            let path = runtime.join(directory).join(format!("discord-ipc-{index}"));
            if let Ok(stream) = UnixStream::connect(&path) {
                return Some(stream);
            }
        }
    }
    None
}

/// one IPC frame: little-endian opcode, length, JSON.
fn frame(op: u32, payload: &serde_json::Value) -> Vec<u8> {
    let body = payload.to_string();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_op_length_json() {
        let payload = serde_json::json!({ "v": 1 });
        let bytes = frame(0, &payload);
        assert_eq!(&bytes[..4], &0u32.to_le_bytes());
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
            bytes.len() - 8
        );
        assert_eq!(&bytes[8..], b"{\"v\":1}");
    }

    #[test]
    fn activity_carries_cover_and_button() {
        let activity = Activity {
            details: "Blox Fruits".into(),
            state: "via Roséline".into(),
            started: 1_700_000_000,
            large_image: Some("https://tr.rbxcdn.com/cover".into()),
            small_image: Some(ROBLOX_LOGO.into()),
            button: Some(Button {
                label: "Join Game",
                url: "roblox://experiences/start?placeId=1&gameInstanceId=e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b"
                    .into(),
            }),
        };
        assert_eq!(activity.details, "Blox Fruits");
        assert!(activity.large_image.is_some());
        assert!(activity.small_image.as_deref().unwrap().contains("Roblox"));
        let button = activity.button.as_ref().unwrap();
        assert_eq!(button.label, "Join Game");
        assert!(button.url.starts_with("roblox://experiences/start?"));
    }
}
