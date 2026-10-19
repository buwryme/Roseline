// SPDX-License-Identifier: GPL-3.0-or-later

//! the guest's logs: game joins and startup milestones.
//!
//! markers follow Bloxstrap's watcher (MIT, the Bloxstrap contributors).

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::SystemTime;

use crate::config::Paths;

/// a join seen but not yet named.
pub struct Joining {
    pub place_id: u64,
    pub job_id: String,
}

impl Joining {
    /// web link that starts this server; Discord buttons only take http(s).
    pub fn join_url(&self) -> String {
        format!(
            "https://www.roblox.com/games/start?placeId={}&gameInstanceId={}",
            self.place_id, self.job_id
        )
    }
}

pub enum Event {
    /// a join line appeared; the name and cover still need fetching.
    Joining(Joining),
    /// a startup milestone, as a percentage and a caption.
    Progress(u8, &'static str),
    /// back at the desktop app, or disconnected.
    Left,
}

pub struct Session {
    started: SystemTime,
    log: Option<PathBuf>,
    offset: u64,
}

impl Session {
    pub fn new() -> Session {
        Session {
            started: SystemTime::now(),
            log: None,
            offset: 0,
        }
    }

    /// read whatever the guest wrote since the last tick.
    pub fn tick(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(text) = self.read_new() else {
            return events;
        };
        for line in text.lines() {
            if line.contains("! Joining game '") {
                if let Some((job_id, place_id)) = parse_join(line) {
                    events.push(Event::Joining(Joining { place_id, job_id }));
                }
            } else if line.contains("Replicator created:") {
                events.push(Event::Progress(97, "Joining the server"));
            } else if line.contains("Time to disconnect replication data:")
                || line.contains("leaveUGCGameInternal")
            {
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
        events
    }

    /// the newest log written since this session began.
    fn read_new(&mut self) -> Option<String> {
        let log = match &self.log {
            Some(log) => log.clone(),
            None => {
                let found = self.locate()?;
                self.log = Some(found.clone());
                found
            }
        };
        let mut file = std::fs::File::open(&log).ok()?;
        let length = file.metadata().ok()?.len();
        if length < self.offset {
            self.offset = 0;
        }
        file.seek(SeekFrom::Start(self.offset)).ok()?;
        let mut text = String::new();
        file.read_to_string(&mut text).ok()?;
        self.offset = length;
        Some(text)
    }

    fn locate(&self) -> Option<PathBuf> {
        let directory = guest_logs_dir()?;
        let mut newest: Option<(SystemTime, PathBuf)> = None;
        for entry in std::fs::read_dir(&directory).ok()?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.contains("Player") || !name.ends_with(".log") {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
                continue;
            };
            // a file from the previous run would only confuse the state.
            let age_known = self
                .started
                .duration_since(modified)
                .map(|age| age.as_secs() < 30)
                .unwrap_or(true);
            if !age_known {
                continue;
            }
            if newest.as_ref().is_none_or(|(time, _)| *time < modified) {
                newest = Some((modified, entry.path()));
            }
        }
        newest.map(|(_, path)| path)
    }
}

/// the guest's `~/Library/Logs/Roblox`, through the prefix.
fn guest_logs_dir() -> Option<PathBuf> {
    let directory = Paths::get().guest_user()?.join("Library/Logs/Roblox");
    directory.is_dir().then_some(directory)
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
    fn join_url_is_a_web_link() {
        let joining = Joining {
            place_id: 4483381587,
            job_id: "e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b".into(),
        };
        assert_eq!(
            joining.join_url(),
            "https://www.roblox.com/games/start?placeId=4483381587&gameInstanceId=e42cc7b0-9b0a-4b7e-8f35-1c2d3e4f5a6b"
        );
    }
}
