// SPDX-License-Identifier: GPL-3.0-or-later

//! what a launch reports to its window while it works.

use std::process::Child;

use async_channel::Sender;

pub enum Event {
    /// what the pipeline is doing, in one line.
    Stage(String),
    /// download or unpack fraction, 0.0 to 1.0.
    Progress(f64),
    /// the game is up.
    Spawned(Spawned),
    /// everything before the game is in place; setup stops here.
    Done,
    /// the pipeline stopped; the text says why.
    Failed(String),
}

pub struct Spawned {
    pub child: Child,
    pub rpc: Option<crate::rpc::Discord>,
    pub pid: u32,
}

/// the sending half; cloneable, used off the main thread.
#[derive(Clone)]
pub struct Progress {
    sender: Sender<Event>,
}

impl Progress {
    pub fn new(sender: Sender<Event>) -> Progress {
        Progress { sender }
    }

    /// a sink for code paths with nowhere to report.
    pub fn silent() -> Progress {
        let (sender, receiver) = async_channel::bounded(1);
        drop(receiver);
        Progress { sender }
    }

    pub fn stage(&self, text: impl Into<String>) {
        let _ = self.sender.send_blocking(Event::Stage(text.into()));
    }

    pub fn progress(&self, fraction: f64) {
        let _ = self
            .sender
            .send_blocking(Event::Progress(fraction.clamp(0.0, 1.0)));
    }

    pub fn spawned(&self, spawned: Spawned) {
        let _ = self.sender.send_blocking(Event::Spawned(spawned));
    }

    pub fn done(&self) {
        let _ = self.sender.send_blocking(Event::Done);
    }

    pub fn failed(&self, text: impl Into<String>) {
        let _ = self.sender.send_blocking(Event::Failed(text.into()));
    }
}
