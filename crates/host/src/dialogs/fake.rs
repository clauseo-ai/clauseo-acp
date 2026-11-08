//! [`FakeDialogs`]: the native dialogs answered from a script, for tests.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use super::{Dialog, DialogAnswer, NativeDialogs, Pick};

/// Answers each [`NativeDialogs::ask`] with the next scripted answer and each
/// [`NativeDialogs::pick`] with the next scripted pick, and records what it was asked. Past the
/// script's end it answers as an Escape would: `Refuse`, and no pick.
#[derive(Debug, Default)]
pub struct FakeDialogs {
    script: Mutex<Script>,
}

#[derive(Default)]
struct Script {
    answers: VecDeque<DialogAnswer>,
    picks: VecDeque<Option<PathBuf>>,
    asked: Vec<Dialog>,
    picked: Vec<Pick>,
}

impl FakeDialogs {
    /// A fake giving `answers`, in order, and no picks.
    pub fn new(answers: impl IntoIterator<Item = DialogAnswer>) -> Self {
        let script = Script {
            answers: answers.into_iter().collect(),
            ..Script::default()
        };
        Self {
            script: Mutex::new(script),
        }
    }

    /// The same, giving `picks`, in order (`None` for a cancelled picker).
    #[must_use]
    pub fn with_picks(self, picks: impl IntoIterator<Item = Option<PathBuf>>) -> Self {
        self.lock().picks = picks.into_iter().collect();
        self
    }

    /// The dialogs asked so far, in order.
    pub fn asked(&self) -> Vec<Dialog> {
        self.lock().asked.clone()
    }

    /// The pickers opened so far, in order.
    pub fn picked(&self) -> Vec<Pick> {
        self.lock().picked.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Script> {
        self.script.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Counts only: no path or name.
impl std::fmt::Debug for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Script")
            .field("answers", &self.answers)
            .field("picks", &self.picks.len())
            .field("asked", &self.asked.len())
            .field("picked", &self.picked.len())
            .finish()
    }
}

impl NativeDialogs for FakeDialogs {
    fn ask(&self, dialog: Dialog) -> DialogAnswer {
        let mut script = self.lock();
        script.asked.push(dialog);
        script.answers.pop_front().unwrap_or(DialogAnswer::Refuse)
    }

    fn pick(&self, pick: Pick) -> Option<PathBuf> {
        let mut script = self.lock();
        script.picked.push(pick);
        script.picks.pop_front().flatten()
    }
}

#[cfg(test)]
mod tests;
