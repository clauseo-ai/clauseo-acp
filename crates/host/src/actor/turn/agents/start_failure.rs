//! A launch that failed after the agent started ([`launch_failed`]), and the log line about an
//! agent that ended before its session was ready ([`log_start_tail`]).
//!
//! The adapter prints its session-start phases to stderr (claude-agent-acp 0.89.1:
//! `[session/create] sessionId=… phase=validate-cwd durationMs=1 totalMs=1`, then `settings`,
//! `prepare-query`, `sdk-initialize`, …), so the phases it reached tell where a start failed.
//! Decision record 0047 keeps agent output, stderr tails included, out of the log file, so the
//! line holds only those phase lines (ids, phase names and durations) and the tail's last line,
//! the one most likely to name the failure; the process crate has already redacted the tail
//! (the home folder as `~`, secret values, `sk-` words, bearer tokens), and here the matter
//! folder, and any path below it, becomes [`MATTER`] ([`matter_spellings`]): a folder's name
//! often names the client. At most [`START_TAIL_BYTES`] of it go in; it never goes to an event
//! or the wire. Decision record 0047 lists this line as its one exception.

use std::path::{Path, PathBuf};

use clauseo_protocol::{DetachReason, ErrorCode, ThreadId};

use super::{Stage, fail_waiting, state};
use crate::actor::Actor;
use crate::error::Error;
use crate::log_file::Cause;

/// The most of the stderr summary the log line holds.
pub(crate) const START_TAIL_BYTES: usize = 2048;

/// What the matter folder, or a path below it, becomes in the summary.
pub(crate) const MATTER: &str = "<matter>";

/// macOS's top folders that are links into `/private`: a path below one of them may be
/// printed with `/private` in front, or without it.
const MAC_PRIVATE_LINKS: [&str; 3] = ["/var/", "/tmp/", "/etc/"];

/// The prefix of claude-agent-acp's session-start phase lines.
const PHASE_LINE: &str = "[session/create] ";

/// The launch failed after the agent started: stop it and fail what waits for it. An error the
/// agent answered is logged with the request's method, its code, its message's length and its
/// `data`'s key names, never the message or a value. The agent's end then logs its stderr
/// summary ([`log_start_tail`]).
pub(in super::super) fn launch_failed(actor: &mut Actor, thread: ThreadId, error: &Error) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    if let Error::Acp(clauseo_acp::Error::Rpc {
        method,
        code,
        message,
        data,
    }) = error
    {
        tracing::warn!(
            %thread,
            error = %Cause(error),
            method = %method,
            code,
            message_len = message.len(),
            data = %data,
            "the agent did not start: it answered a startup request with an error"
        );
    } else {
        tracing::warn!(%thread, error = %Cause(error), "the agent did not start");
    }
    // Claude's "`cwd` does not exist" answer to `session/new` is `matter.folder_permission`.
    let refusal = crate::agent::folder_access::session_refusal(error);
    let error = refusal.as_ref().unwrap_or(error);
    agent.link.stop();
    agent.stage = Stage::Stopping;
    agent.start_failure = Some(error.code());
    let waiting = std::mem::take(&mut agent.waiting);
    for item in waiting {
        fail_waiting(actor, thread, &item, error);
    }
}

/// Logs, at WARN, the stderr summary of an agent that ended before its session was ready
/// ([`start_summary`], with `matter` its matter folder's spellings), with the code its launch
/// failed with (`None` when it ended on its own); nothing when `reason` carries no stderr tail.
pub(super) fn log_start_tail(
    thread: ThreadId,
    failure: Option<ErrorCode>,
    reason: &DetachReason,
    matter: &[String],
) {
    let DetachReason::Exited { code, stderr_tail } = reason else {
        return;
    };
    let summary = start_summary(stderr_tail, matter);
    tracing::warn!(
        %thread,
        failure = failure.as_ref().map_or("none", ErrorCode::as_str),
        ?code,
        stderr = %summary,
        "the agent ended before its session was ready; its start phases and last stderr line"
    );
}

/// The phase lines of `tail` and its last non-empty line (once, when it is a phase line), each
/// with the matter folder's spellings (`matter`, [`matter_spellings`]) and any path below them
/// as [`MATTER`] ([`without_matter`]), joined with ` | `, cut to [`START_TAIL_BYTES`] on a
/// character boundary after that; `none` for an empty tail.
pub(crate) fn start_summary(tail: &str, matter: &[String]) -> String {
    let lines: Vec<&str> = tail
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    let mut kept: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| line.starts_with(PHASE_LINE) && line.contains(" phase="))
        .collect();
    if let Some(last) = lines.last()
        && kept.last() != Some(last)
    {
        kept.push(last);
    }
    if kept.is_empty() {
        return "none".to_owned();
    }
    let mut summary = kept
        .iter()
        .map(|line| without_matter(line, matter))
        .collect::<Vec<_>>()
        .join(" | ");
    if summary.len() > START_TAIL_BYTES {
        let mut end = START_TAIL_BYTES;
        while !summary.is_char_boundary(end) {
            end -= 1;
        }
        summary.truncate(end);
    }
    summary
}

/// [`matter_spellings`] of the matter folder an agent launches in: as the host stored it
/// (`stored`) and as its working folder (`session`, canonical), with the actor's home spellings,
/// on macOS with the `/private` forms. Reads no disk: the caller has canonicalised already.
pub(super) fn spellings_at_launch(actor: &Actor, stored: &Path, session: &Path) -> Vec<String> {
    matter_spellings(
        &[stored, session],
        &actor.home_spellings,
        cfg!(target_os = "macos"),
    )
}

/// The spellings of a matter folder its agent's stderr may print, longest first: each of
/// `folders` (the folder as the host stored it, as given, and as the agent's working folder,
/// canonical, with the case on disk), without a trailing separator; on macOS (`mac`) each also
/// with or without the `/private` in front of `/var`, `/tmp` and `/etc`; each of those with
/// a home spelling (`homes`) in it as `~`, as the process crate's redaction left it; and on
/// Windows each with its `\` doubled, as JSON prints it. A spelling shorter than two bytes (a
/// root) is left out: it would hide every path.
pub(crate) fn matter_spellings(folders: &[&Path], homes: &[PathBuf], mac: bool) -> Vec<String> {
    let mut plain: Vec<String> = Vec::new();
    for folder in folders {
        let Some(text) = folder.to_str() else {
            continue;
        };
        let text = text.trim_end_matches(['/', '\\']);
        plain.push(text.to_owned());
        if mac {
            if let Some(rest) = text.strip_prefix("/private")
                && MAC_PRIVATE_LINKS.iter().any(|link| rest.starts_with(link))
            {
                plain.push(rest.to_owned());
            } else if MAC_PRIVATE_LINKS.iter().any(|link| text.starts_with(link)) {
                plain.push(format!("/private{text}"));
            }
        }
    }
    let mut spellings = plain.clone();
    for text in &plain {
        for home in homes.iter().filter_map(|home| home.to_str()) {
            let home = home.trim_end_matches(['/', '\\']);
            if !home.is_empty() && text.contains(home) {
                spellings.push(text.replace(home, "~"));
            }
        }
    }
    let doubled: Vec<String> = spellings
        .iter()
        .filter(|text| text.contains('\\'))
        .map(|text| text.replace('\\', "\\\\"))
        .collect();
    spellings.extend(doubled);
    spellings.retain(|text| text.len() >= 2);
    spellings.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    spellings.dedup();
    spellings
}

/// `line` with each of `matter`'s spellings (longest first), and the rest of the path it
/// starts, as [`MATTER`]. The path runs to the matching quote when a quote opens it, else to
/// the first space, quote, bracket, `,`, `;`, `:` or `|`. So `'<folder>/Draft 2.docx'` goes
/// whole; an unquoted path with a space in it keeps what follows the space.
pub(crate) fn without_matter(line: &str, matter: &[String]) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        let found = matter
            .iter()
            .filter_map(|spelling| rest.find(spelling.as_str()).map(|at| (at, spelling.len())))
            .min_by_key(|&(at, len)| (at, std::cmp::Reverse(len)));
        let Some((at, len)) = found else {
            out.push_str(rest);
            return out;
        };
        let before = rest.get(..at).unwrap_or_default();
        let tail = rest.get(at + len..).unwrap_or_default();
        let opener = before
            .chars()
            .next_back()
            .filter(|c| matches!(c, '\'' | '"' | '`'));
        let end = match opener {
            Some(quote) => tail.find(quote),
            None => tail.find(ends_path),
        }
        .unwrap_or(tail.len());
        out.push_str(before);
        out.push_str(MATTER);
        rest = tail.get(end..).unwrap_or_default();
    }
}

/// Whether `c` ends an unquoted path in a stderr line ([`without_matter`]).
fn ends_path(c: char) -> bool {
    c.is_whitespace() || "'\"`()[]{}<>,;:|".contains(c)
}

#[cfg(test)]
mod tests;
