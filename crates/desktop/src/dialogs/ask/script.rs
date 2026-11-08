//! The `e2e` build's native dialogs: answers and picks from a script in its variables, so the
//! end-to-end tests widen a matter without a person at the dialog.
//!
//! - `E2E_DIALOG_ANSWERS`: comma-separated answers, given in order: `allow_once`,
//!   `allow_for_matter`, `refuse` or `refuse_for_matter`; anything else refuses.
//! - `E2E_DIALOG_PICKS`: the picks, in order, separated by `|` (no Windows path holds one); an
//!   empty entry is a cancelled picker.
//!
//! Past the script's end every question refuses and every picker is cancelled, as Escape would.
//! Either variable set and not empty scripts both; neither draws the native dialogs.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use clauseo_host::{Dialog, DialogAnswer, NativeDialogs, Pick};

/// The scripted answers, in order.
#[cfg(feature = "e2e")]
const ANSWERS_VAR: &str = "E2E_DIALOG_ANSWERS";

/// The scripted picks, in order.
#[cfg(feature = "e2e")]
const PICKS_VAR: &str = "E2E_DIALOG_PICKS";

/// The dialogs answered from a script.
pub(super) struct Scripted {
    answers: Mutex<VecDeque<DialogAnswer>>,
    picks: Mutex<VecDeque<Option<PathBuf>>>,
}

impl Scripted {
    /// The script in the variables, when either is set and not empty.
    #[cfg(feature = "e2e")]
    pub(super) fn from_env() -> Option<Self> {
        let answers = std::env::var(ANSWERS_VAR).ok().filter(|v| !v.is_empty());
        let picks = std::env::var(PICKS_VAR).ok().filter(|v| !v.is_empty());
        if answers.is_none() && picks.is_none() {
            return None;
        }
        let picks = picks.map(|list| list.split('|').map(PathBuf::from).collect::<Vec<_>>());
        Some(Self::new(answers.as_deref(), picks.unwrap_or_default()))
    }

    /// The script: `answers` as `E2E_DIALOG_ANSWERS` holds them, and `picks` (an empty path is
    /// a cancelled picker).
    pub(super) fn new(answers: Option<&str>, picks: Vec<PathBuf>) -> Self {
        let answers = answers
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|word| !word.is_empty())
            .map(answer)
            .collect();
        let picks = picks
            .into_iter()
            .map(|path| Some(path).filter(|path| !path.as_os_str().is_empty()))
            .collect();
        Self {
            answers: Mutex::new(answers),
            picks: Mutex::new(picks),
        }
    }
}

/// The answers left, and the number of picks: no path.
impl std::fmt::Debug for Scripted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        let picks = self.picks.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("Scripted")
            .field("answers", &*answers)
            .field("picks", &picks.len())
            .finish()
    }
}

/// The answer `word` names; anything else refuses.
fn answer(word: &str) -> DialogAnswer {
    match word {
        "allow_once" => DialogAnswer::AllowOnce,
        "allow_for_matter" => DialogAnswer::AllowForMatter,
        "refuse_for_matter" => DialogAnswer::RefuseForMatter,
        _ => DialogAnswer::Refuse,
    }
}

impl NativeDialogs for Scripted {
    fn ask(&self, _dialog: Dialog) -> DialogAnswer {
        let mut answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        answers.pop_front().unwrap_or(DialogAnswer::Refuse)
    }

    fn pick(&self, _pick: Pick) -> Option<PathBuf> {
        let mut picks = self.picks.lock().unwrap_or_else(PoisonError::into_inner);
        picks.pop_front().flatten()
    }
}
