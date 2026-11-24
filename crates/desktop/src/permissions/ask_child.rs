//! The keychain Ask's child process: `Clauseo --keychain-ask`, dispatched before Tauri, logging
//! and settings (`jailed.rs`'s `launch`). The app's own process keeps keychain dialogs off for
//! its whole life (`quiet.rs`); this child starts with them on (it never turns them off), so
//! macOS's dialog shows here and only while the lawyer's Ask runs.
//!
//! Protocol, one JSON object per line:
//! - stdin: `{"service":…,"account":…}` for each item to read; the child reads stdin to its end
//!   first, then reads the items one by one (macOS shows one dialog per item).
//! - stdout: per item, in order, `{"service":…,"account":…,"outcome":"read"|"gone"|"refused"}`,
//!   with `"code":<OSStatus>` on a refusal. Never a value: each one read is zeroed at once
//!   (`mac.rs`). Then it exits 0; a line it cannot parse is skipped.
//!
//! The parsing and the writing are pure ([`requests`], [`serve`], [`results`]), so the protocol
//! is tested anywhere; the app's side starts the child through the process crate
//! (`system.rs`).

use std::ffi::OsStr;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

use super::KeychainItem;

/// The flag that starts the child.
pub(crate) const FLAG: &str = "--keychain-ask";

/// Whether `arg`, the first argument after the program, asks for the child.
pub(crate) fn requested(arg: &OsStr) -> bool {
    arg == FLAG
}

/// One item to read: a line of the child's stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AskItem {
    pub(crate) service: String,
    pub(crate) account: String,
}

/// What reading an item came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Outcome {
    /// It read (the lawyer allowed it, once or always).
    Read,
    /// It has gone: nothing left to ask for.
    Gone,
    /// The read failed: the lawyer denied or cancelled, or another error (`code`).
    Refused,
}

/// One item's answer: a line of the child's stdout. Never a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AskResult {
    pub(crate) service: String,
    pub(crate) account: String,
    pub(crate) outcome: Outcome,
    /// The OS's code, on a refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) code: Option<i32>,
}

impl AskResult {
    /// The answer for `item` from its read: `Ok(true)` read, `Ok(false)` gone, `Err(code)`.
    pub(crate) fn of(item: AskItem, read: Result<bool, i32>) -> Self {
        let (outcome, code) = match read {
            Ok(true) => (Outcome::Read, None),
            Ok(false) => (Outcome::Gone, None),
            Err(code) => (Outcome::Refused, Some(code)),
        };
        Self {
            service: item.service,
            account: item.account,
            outcome,
            code,
        }
    }

    /// Whether it answers `item`.
    pub(crate) fn is_for(&self, item: &KeychainItem) -> bool {
        self.service == item.service && self.account == item.account
    }
}

/// The child's stdin for `items`: one line each.
pub(crate) fn request_lines(items: &[KeychainItem]) -> String {
    items
        .iter()
        .map(|item| {
            let line = AskItem {
                service: item.service.clone(),
                account: item.account.clone(),
            };
            // A struct of two strings always serializes.
            serde_json::to_string(&line).unwrap_or_default() + "\n"
        })
        .collect()
}

/// The items in the child's stdin, skipping lines that do not parse (and blank ones).
pub(crate) fn requests(input: impl BufRead) -> Vec<AskItem> {
    input
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

/// The child's work: reads the items in `input` to its end, then each with `read`, writing
/// one line per item to `output` as it answers. Its exit code: 0, or 1 when `output` closed.
pub(crate) fn serve(
    input: impl BufRead,
    mut output: impl Write,
    mut read: impl FnMut(&AskItem) -> Result<bool, i32>,
) -> i32 {
    for item in requests(input) {
        let answer = read(&item);
        let line = serde_json::to_string(&AskResult::of(item, answer)).unwrap_or_default();
        if writeln!(output, "{line}")
            .and_then(|()| output.flush())
            .is_err()
        {
            // The app has gone: nobody to answer.
            return 1;
        }
    }
    0
}

/// The answers in the child's stdout, skipping lines that do not parse.
pub(crate) fn results(output: &str) -> Vec<AskResult> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The child itself, on this process's stdin and stdout; its exit code.
pub(crate) fn run() -> i32 {
    serve(io::stdin().lock(), io::stdout().lock(), |item| {
        super::system::read(&item.service, &item.account)
    })
}
