//! The fake machine's side of `--wsl-deny`'s schedule (`wsl_phase/schedule.rs`): a clock that
//! only waits move, WSL's `AppID` keys changing on a timeline, the one-shot change notification
//! (fired by a change while armed, as `RegNotifyChangeKeyValue`), and Windows Installer's
//! `InProgress` and mutex as intervals.

use std::time::Duration;

use super::fake::Fake;
use super::{Failed, Installer, WslWatch};

/// A change to WSL's `AppID` keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyEvent {
    /// The installer deletes them.
    Delete,
    /// The installer writes them again, with stock permissions: no deny.
    Rewrite,
    /// A change that leaves the deny as it is (another `AppID`'s key, say).
    Touch,
    /// WSL is uninstalled: its keys are gone, and nothing names them any more.
    Uninstall,
}

/// WSL's `AppID` keys, when a test models them; `reapply_wsl` then reads and writes them.
#[derive(Debug)]
pub(super) struct Keys {
    /// The changes still to come, in time order.
    pub(super) events: Vec<(Duration, KeyEvent)>,
    /// Changes made while a pass reads, at its first pass at or after the time: the pass sees
    /// the keys from before.
    pub(super) mid_read: Vec<(Duration, KeyEvent)>,
    pub(super) present: bool,
    pub(super) denied: bool,
    /// WSL is uninstalled: a pass finds no targets.
    pub(super) uninstalled: bool,
    /// Since when a rewrite has left the deny off.
    off_since: Option<Duration>,
    /// How long each rewrite left the deny off, once a pass put it back.
    pub(super) off: Vec<Duration>,
}

impl Keys {
    /// Keys there and denied, with `events` to come.
    pub(super) fn new(events: Vec<(Duration, KeyEvent)>) -> Keys {
        Keys {
            events,
            mid_read: Vec::new(),
            present: true,
            denied: true,
            uninstalled: false,
            off_since: None,
            off: Vec::new(),
        }
    }

    fn apply(&mut self, event: KeyEvent, at: Duration) {
        match event {
            KeyEvent::Delete => {
                (self.present, self.denied, self.off_since) = (false, false, None);
            }
            KeyEvent::Rewrite => {
                (self.present, self.denied) = (true, false);
                self.off_since.get_or_insert(at);
            }
            KeyEvent::Touch => {}
            KeyEvent::Uninstall => {
                (self.present, self.denied, self.off_since) = (false, false, None);
                self.uninstalled = true;
            }
        }
    }
}

/// See the module.
#[derive(Debug, Default)]
pub(super) struct FakeWatch {
    pub(super) clock: Duration,
    /// The waits `wait_change` was asked for.
    pub(super) waits: Vec<Duration>,
    /// `None`: `reapply_wsl` gives `wsl_passes`, then `wsl`, and nothing changes.
    pub(super) keys: Option<Keys>,
    /// When `InProgress` is there, `[from, to)`.
    pub(super) in_progress: Vec<(Duration, Duration)>,
    /// When the mutex is there, `[from, to)`.
    pub(super) mutex: Vec<(Duration, Duration)>,
    /// A change every millisecond until then.
    pub(super) storm_until: Duration,
    /// Arming fails.
    pub(super) arm_fails: bool,
    /// When each pass read the keys.
    pub(super) passes: Vec<Duration>,
    armed: bool,
    signalled: bool,
}

/// Whether `at` lies in one of `spans`.
fn within(spans: &[(Duration, Duration)], at: Duration) -> bool {
    spans.iter().any(|(from, to)| *from <= at && at < *to)
}

impl FakeWatch {
    /// A change: it fires the notification when one is armed.
    fn change(&mut self) {
        if self.armed {
            (self.armed, self.signalled) = (false, true);
        }
    }

    /// The next key change at or before `until`, if any.
    fn next_event(&self, until: Duration) -> Option<Duration> {
        let keys = self.keys.as_ref()?;
        keys.events
            .first()
            .map(|(at, _)| *at)
            .filter(|at| *at <= until)
    }

    /// Moves the clock to `to`, applying the key changes on the way.
    pub(super) fn advance(&mut self, to: Duration) {
        while let Some(at) = self.next_event(to) {
            self.clock = self.clock.max(at);
            if let Some(keys) = self.keys.as_mut() {
                let (_, event) = keys.events.remove(0);
                keys.apply(event, at);
            }
            self.change();
        }
        self.clock = self.clock.max(to);
    }

    /// A pass's read and re-apply, when the keys are modelled.
    pub(super) fn reapply(&mut self) -> Option<Result<Option<String>, Failed>> {
        let clock = self.clock;
        self.passes.push(clock);
        let keys = self.keys.as_mut()?;
        let outcome = if keys.uninstalled {
            Ok(None)
        } else if !keys.present {
            Err(
                "{370121D2-AA7E-4608-A86D-0BBAB9DA1A60}: a WSL class names this AppID, whose \
                 key isn't there"
                    .to_owned(),
            )
        } else if keys.denied {
            Ok(None)
        } else {
            keys.denied = true;
            if let Some(since) = keys.off_since.take() {
                keys.off.push(clock.saturating_sub(since));
            }
            Ok(Some(
                "WSL's deny for the sandbox users was put back on 4 of 4 launch permissions"
                    .to_owned(),
            ))
        };
        let wrote = matches!(outcome, Ok(Some(_)));
        let during: Vec<KeyEvent> = keys
            .mid_read
            .iter()
            .filter(|(at, _)| *at <= clock)
            .map(|(_, event)| *event)
            .collect();
        keys.mid_read.retain(|(at, _)| *at > clock);
        for event in &during {
            keys.apply(*event, clock);
        }
        // The pass's own write, and a change made while it read, are changes too.
        if wrote || !during.is_empty() {
            self.change();
        }
        Some(outcome)
    }
}

impl WslWatch for Fake {
    fn now(&mut self) -> Duration {
        self.watch.clock
    }

    fn watch(&mut self) -> Result<(), Failed> {
        self.calls.push("watch");
        if self.watch.arm_fails {
            return Err("RegNotifyChangeKeyValue failed (5)".to_owned());
        }
        // As the real one: an armed, unfired notification stays; a fired one is reset.
        if !self.watch.armed {
            (self.watch.armed, self.watch.signalled) = (true, false);
        }
        Ok(())
    }

    fn wait_change(&mut self, time: Duration) -> bool {
        self.calls.push("wait_change");
        let watch = &mut self.watch;
        watch.waits.push(time);
        let until = watch.clock.saturating_add(time);
        loop {
            if watch.armed && watch.clock < watch.storm_until {
                watch.advance(watch.clock.saturating_add(Duration::from_millis(1)));
                watch.change();
            }
            if watch.signalled {
                watch.signalled = false;
                return true;
            }
            if watch.clock >= until {
                return false;
            }
            let next = watch.next_event(until).unwrap_or(until);
            watch.advance(next);
        }
    }

    fn recorded_key_missing(&mut self) -> Result<bool, Failed> {
        self.calls.push("recorded_key_missing");
        Ok(self.watch.keys.as_ref().is_some_and(|keys| !keys.present))
    }

    fn wsl_present(&mut self) -> bool {
        self.calls.push("wsl_present");
        !self
            .watch
            .keys
            .as_ref()
            .is_some_and(|keys| keys.uninstalled)
    }

    fn installer(&mut self) -> Installer {
        self.calls.push("installer");
        Installer {
            in_progress: within(&self.watch.in_progress, self.watch.clock),
            mutex: within(&self.watch.mutex, self.watch.clock),
        }
    }
}
