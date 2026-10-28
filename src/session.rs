// SPDX-License-Identifier: GPL-3.0-or-later

//! the guest's logs: game joins and startup milestones.
//!
//! markers follow Bloxstrap's watcher (MIT, the Bloxstrap contributors).

use std::io::{Read, Seek, SeekFrom};
use std::net::IpAddr;
use std::path::PathBuf;

/// a join seen but not yet named.
pub struct Joining {
    pub place_id: u64,
    pub job_id: String,
    pub server_ip: Option<IpAddr>,
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
    Returned,
}

struct Pending {
    place_id: u64,
    job_id: String,
    server_ip: Option<IpAddr>,
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
            if let Some((job_id, place_id, server_ip)) = parse_join(line) {
                self.pending = Some(Pending {
                    place_id,
                    job_id,
                    server_ip,
                    private: false,
                });
            }
        } else if private_referral(line) {
            if let Some(pending) = self.pending.as_mut() {
                pending.private = true;
            }
        } else if line.contains("Replicator created for player ") {
            if let Some(ip) = parse_public_ip(line) {
                if let Some(pending) = self.pending.as_mut() {
                    pending.server_ip = Some(ip);
                }
            }
        } else if line.contains("Replicator created:") {
            // by now the join-time referral has told us public vs private.
            if let Some(pending) = self.pending.take() {
                events.push(Event::Joining(Joining {
                    place_id: pending.place_id,
                    job_id: pending.job_id,
                    server_ip: pending.server_ip,
                    private: pending.private,
                }));
            }
            events.push(Event::Progress(97, "Joining the server"));
        } else if line.contains("returnToLuaApp: (stage:UGCGame)") {
            self.pending = None;
            events.push(Event::Returned);
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
fn parse_join(line: &str) -> Option<(String, u64, Option<IpAddr>)> {
    let rest = line.split("! Joining game '").nth(1)?;
    let job_id = rest.split('\'').next()?;
    let place_id: u64 = rest
        .split(" place ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let server_ip = rest
        .split(" at ")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(|ip| ip.parse().ok())
        .filter(|ip| usable(*ip));
    (job_id.len() == 36).then(|| (job_id.to_string(), place_id, server_ip))
}

fn parse_public_ip(line: &str) -> Option<IpAddr> {
    const MARKERS: [&str; 4] = [
        "UDMUX Address = ",
        "Connected to server at ",
        "serverId: ",
        "Replicator created for player ",
    ];
    MARKERS.iter().find_map(|marker| {
        let rest = line.split(marker).nth(1)?;
        let candidate = rest.split([':', ',', '|', ' ']).next()?;
        let ip: IpAddr = candidate.parse().ok()?;
        usable(ip).then_some(ip)
    })
}

fn usable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified())
        }
        IpAddr::V6(ip) => !(ip.is_loopback() || ip.is_unspecified()),
    }
}

/// a private server join names its referral in the join-load-time line.
fn private_referral(line: &str) -> bool {
    line.contains("referral_page:")
        && (line.contains("RequestPrivateGame") || line.contains("GameDetailPageJSHybridEvent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn join_lines_parse() {
        let line = "2026-10-18T13:49:33.349Z,1.2,abc,6 [FLog::Output] ! Joining game 'e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b' place 2753915549 at 128.116.44.3";
        let (job, place, ip) = parse_join(line).unwrap();
        assert_eq!(job, "e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b");
        assert_eq!(place, 2753915549);
        assert_eq!(ip.unwrap().to_string(), "128.116.44.3");
    }

    #[test]
    fn public_addresses_are_taken_from_the_network_lines() {
        let line = "2026-10-27T14:26:02.557Z,3.557239,ed3c0000,6 [FLog::Network] Replicator created for player 128.116.13.34|59039";
        assert_eq!(parse_public_ip(line).unwrap().to_string(), "128.116.13.34");
        assert_eq!(
            parse_public_ip("[FLog::Network] UDMUX Address = 128.116.44.33, Port = 59435")
                .unwrap()
                .to_string(),
            "128.116.44.33"
        );
        assert!(
            parse_public_ip("[FLog::Network] Replicator created for player 10.30.4.228|59039")
                .is_none()
        );
    }

    #[test]
    fn join_uses_the_public_network_address() {
        let root =
            std::env::temp_dir().join(format!("roseline-session-public-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let log = root.join("player.log");
        std::fs::write(
            &log,
            "\
2026-10-27T14:39:14.326Z,8.326221,eb130000,6 [FLog::Output] ! Joining game 'e6666ab4-889d-4c4e-9e13-57604261e19d' place 142823291 at 10.9.7.196
2026-10-27T14:39:14.944Z,8.944253,eb210000,7 [FLog::Network] Replicator created for player 128.116.44.33|53452
2026-10-27T14:39:14.944Z,8.944260,eb210000,7 [FLog::Network] Replicator created: 0x7f278ba47e40
",
        )
        .unwrap();
        let events = Session::new(log).tick();
        let joining = events
            .iter()
            .find_map(|event| match event {
                Event::Joining(joining) => Some(joining),
                _ => None,
            })
            .expect("a join event");
        assert_eq!(
            joining.server_ip.map(|ip| ip.to_string()).as_deref(),
            Some("128.116.44.33")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn garbage_join_lines_are_ignored() {
        assert!(parse_join("! Joining game '' place at 1.2.3.4").is_none());
        assert!(parse_join("[FLog::Output] nothing to see").is_none());
    }

    #[test]
    fn only_leaving_a_game_counts_as_returning_home() {
        let root = std::env::temp_dir().join(format!("roseline-leave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let log = root.join("player.log");
        std::fs::write(
            &log,
            "2026-10-19T13:37:04.648Z,1.0,abc,6 [FLog::SingleSurfaceApp] returnToLuaApp: (stage:InitializedLuaApp).\n",
        )
        .unwrap();
        let mut session = Session {
            log: Some(log.clone()),
            offset: 0,
            partial: String::new(),
            pending: None,
        };
        let events = session.tick();
        assert!(!events.iter().any(|event| matches!(event, Event::Returned)));

        let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(
            file,
            "2026-10-24T10:47:44.680Z,34.0,abc,6 [FLog::SingleSurfaceApp] returnToLuaApp: (stage:UGCGame)."
        )
        .unwrap();
        drop(file);
        let events = session.tick();
        assert!(events.iter().any(|event| matches!(event, Event::Returned)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn join_url_uses_the_browser_page() {
        let joining = Joining {
            place_id: 4483381587,
            job_id: "e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b".into(),
            server_ip: Some("128.116.44.3".parse().unwrap()),
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
