// SPDX-License-Identifier: GPL-3.0-or-later

//! the guest's logs: game joins and startup milestones.
//!
//! markers follow Bloxstrap's watcher (MIT, the Bloxstrap contributors).

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// a join seen but not yet named.
pub struct Joining {
    pub place_id: u64,
    pub job_id: String,
    /// private servers cannot be joined with a deep link; use the game page.
    pub private: bool,
}

impl Joining {
    /// https landing page; Discord's Flatpak build only opens http(s), so the
    /// browser gets the deep link instead of the client's own link handler.
    pub fn join_url(&self) -> String {
        format!(
            "{}?placeId={}&gameInstanceId={}",
            crate::config::JOIN_PAGE,
            self.place_id,
            self.job_id
        )
    }

    /// the game's public page, for the View Game button.
    pub fn page_url(&self) -> String {
        format!("https://www.roblox.com/games/{}", self.place_id)
    }
}

pub enum Event {
    /// the server is actually joined; the name and cover still need fetching.
    Joining(Joining),
    /// a startup milestone, as a percentage and a caption.
    Progress(u8, &'static str),
    /// back at the desktop app, or disconnected.
    Left,
}

struct Pending {
    place_id: u64,
    job_id: String,
    private: bool,
}

pub struct Session {
    log: Option<PathBuf>,
    offset: u64,
    partial: String,
    pending: Option<Pending>,
}

impl Session {
    pub fn new(log: PathBuf) -> Session {
        Session {
            log: Some(log),
            offset: 0,
            partial: String::new(),
            pending: None,
        }
    }

    /// read whatever the guest wrote since the last tick.
    pub fn tick(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(text) = self.read_new() else {
            return events;
        };
        self.partial.push_str(&text);
        while let Some(newline) = self.partial.find('\n') {
            let line = self.partial[..newline].trim_end_matches('\r').to_string();
            self.partial.drain(..=newline);
            self.feed_line(&line, &mut events);
        }
        events
    }

    fn feed_line(&mut self, line: &str, events: &mut Vec<Event>) {
        if line.contains("! Joining game '") {
            if let Some((job_id, place_id)) = parse_join(line) {
                self.pending = Some(Pending {
                    place_id,
                    job_id,
                    private: false,
                });
            }
        } else if private_referral(line) {
            if let Some(pending) = self.pending.as_mut() {
                pending.private = true;
            }
        } else if line.contains("Replicator created:") {
            // by now the join-time referral has told us public vs private.
            if let Some(pending) = self.pending.take() {
                events.push(Event::Joining(Joining {
                    place_id: pending.place_id,
                    job_id: pending.job_id,
                    private: pending.private,
                }));
            }
            events.push(Event::Progress(97, "Joining the server"));
        } else if line.contains("Time to disconnect replication data:")
            || line.contains("leaveUGCGameInternal")
        {
            self.pending = None;
            events.push(Event::Left);
        } else if line.contains("applicationDidFinishLaunching") {
            events.push(Event::Progress(62, "Roblox is starting"));
        } else if line.contains("Universal App on Mac: initialize") {
            events.push(Event::Progress(68, "Initialising the app"));
        } else if line.contains("setAssetFolder") {
            events.push(Event::Progress(74, "Loading content"));
        } else if line.contains("initializeWithAppStarter")
            || line.contains("instantiate controllers")
        {
            events.push(Event::Progress(82, "Opening the window"));
        } else if line.contains("Retrieving patch") {
            events.push(Event::Progress(78, "Patching the engine"));
        } else if line.contains("Hello world, CLI") {
            events.push(Event::Progress(86, "Engine is up"));
        } else if line.contains("updateSurfaceLuaApp") {
            // fires as the app swaps in its window surface
            events.push(Event::Progress(92, "Opening the window"));
        }
    }

    /// the new bytes the child wrote to its launch log since the last tick.
    fn read_new(&mut self) -> Option<String> {
        let log = self.log.as_ref()?.clone();
        let mut file = std::fs::File::open(&log).ok()?;
        let length = file.metadata().ok()?.len();
        if length < self.offset {
            self.offset = 0;
        }
        file.seek(SeekFrom::Start(self.offset)).ok()?;
        let mut text = String::new();
        file.read_to_string(&mut text).ok()?;
        self.offset += text.len() as u64;
        Some(text)
    }
}

/// `! Joining game '<36-char uuid>' place <digits> at <ip>`.
fn parse_join(line: &str) -> Option<(String, u64)> {
    let rest = line.split("! Joining game '").nth(1)?;
    let job_id = rest.split('\'').next()?;
    let place_id: u64 = rest
        .split(" place ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    (job_id.len() == 36).then(|| (job_id.to_string(), place_id))
}

/// a private server join names its referral in the join-load-time line.
fn private_referral(line: &str) -> bool {
    line.contains("referral_page:")
        && (line.contains("RequestPrivateGame") || line.contains("GameDetailPageJSHybridEvent"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_lines_parse() {
        let line = "2026-10-18T13:49:33.349Z,1.2,abc,6 [FLog::Output] ! Joining game 'e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b' place 2753915549 at 128.116.44.3";
        let (job, place) = parse_join(line).unwrap();
        assert_eq!(job, "e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b");
        assert_eq!(place, 2753915549);
    }

    #[test]
    fn garbage_join_lines_are_ignored() {
        assert!(parse_join("! Joining game '' place at 1.2.3.4").is_none());
        assert!(parse_join("[FLog::Output] nothing to see").is_none());
    }

    #[test]
    fn join_url_uses_the_browser_page() {
        let joining = Joining {
            place_id: 4483381587,
            job_id: "e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b".into(),
            private: false,
        };
        assert_eq!(
            joining.join_url(),
            "https://buwryy.net/join-roseline-discord-rpc-handler.html?placeId=4483381587&gameInstanceId=e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b"
        );
        assert_eq!(
            joining.page_url(),
            "https://www.roblox.com/games/4483381587"
        );
    }

    #[test]
    fn private_referrals_are_detected() {
        assert!(private_referral(
            "[FLog::GameJoinLoadTime] Report game_join_loadtime: referral_page:RequestPrivateGame,foo"
        ));
        assert!(private_referral(
            "[FLog::GameJoinLoadTime] Report game_join_loadtime: referral_page:GameDetailPageJSHybridEvent,foo"
        ));
        assert!(!private_referral(
            "[FLog::GameJoinLoadTime] Report game_join_loadtime: referral_page:GameDetailPage,foo"
        ));
        assert!(!private_referral("[FLog::Output] nothing to see"));
    }

    #[test]
    fn joining_is_held_until_the_referral_is_known() {
        let root = std::env::temp_dir().join(format!("roseline-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let log = root.join("player.log");
        std::fs::write(
            &log,
            "\
2026-10-18T13:49:33.349Z,1.2,abc,6 [FLog::Output] ! Joining game 'e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b' place 2753915549 at 128.116.44.3
2026-10-18T13:49:33.350Z,1.2,abc,6 [FLog::GameJoinLoadTime] Report game_join_loadtime: referral_page:RequestPrivateGame,foo
2026-10-18T13:49:33.351Z,1.2,abc,6 [FLog::Network] Replicator created: 0x1
",
        )
        .unwrap();
        let mut session = Session {
            log: Some(log),
            offset: 0,
            partial: String::new(),
            pending: None,
        };
        let events = session.tick();
        let joining = events
            .iter()
            .find_map(|event| match event {
                Event::Joining(joining) => Some(joining),
                _ => None,
            })
            .expect("a join event");
        assert!(joining.private);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Progress(97, _)))
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
