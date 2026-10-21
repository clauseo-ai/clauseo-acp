//! Parsers for the text that rustfmt and clippy print, and the findings they become.
//!
//! Clippy runs with `--message-format=json-diagnostic-short`: the short text form (`path:line:col:
//! level: message`) does not name the lint, and the JSON does. Each JSON message carries the short
//! text in `rendered`, which is what people see. A finding for a clippy ban (`BANS`) also gets the
//! notes that give its reason and replacement (see `help_notes`); other lints print their rendered
//! text alone.

use crate::process::Cmd;
use crate::violation::{Finding, Violation};
use crate::workspace::{Context, Member, package_for};

/// Repo-relative, `/`-separated form of a path a tool printed.
/// `root` is the workspace root as the tool printed it (absolute, or `C:\...` on Windows).
pub(crate) fn rel_path(raw: &str, root: &str) -> String {
    let path = raw.trim().replace('\\', "/");
    let root = root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    let rel = path
        .strip_prefix(&format!("{root}/"))
        .unwrap_or(path.as_str());
    rel.strip_prefix("./").unwrap_or(rel).to_owned()
}

/// `(path, line)` for each file `cargo fmt --check` would change. Accepts the current
/// `Diff in <path>:<line>:` header and the older `Diff in <path> at line <line>:` header.
///
/// The header's line is where the hunk's context starts, not the changed line. The reported
/// line is the first removed or added line of the hunk (see `hunk_changed_line`).
pub(crate) fn fmt_diffs(output: &str, root: &str) -> Vec<(String, u32)> {
    let mut found: Vec<(String, u32)> = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix("Diff in ") else {
            continue;
        };
        let parsed = match rest.rsplit_once(" at line ") {
            Some((path, number)) => number
                .trim_end_matches(':')
                .trim()
                .parse::<u32>()
                .ok()
                .map(|n| (path, n)),
            None => rest
                .strip_suffix(':')
                .unwrap_or(rest)
                .rsplit_once(':')
                .and_then(|(path, number)| number.parse::<u32>().ok().map(|n| (path, n))),
        };
        let Some((path, start)) = parsed else {
            continue;
        };
        let body: Vec<&str> = lines[index + 1..]
            .iter()
            .take_while(|l| !l.starts_with("Diff in "))
            .copied()
            .collect();
        let entry = (rel_path(path, root), hunk_changed_line(start, &body));
        if !found.contains(&entry) {
            found.push(entry);
        }
    }
    found
}

/// Line in the original file of the first changed line of a rustfmt hunk: the first removed line,
/// or, for a hunk that only adds lines, the line the first added line goes in front of.
/// Context lines (` `) and removed lines (`-`) both advance the original line number.
fn hunk_changed_line(start: u32, body: &[&str]) -> u32 {
    let mut original = start;
    for line in body {
        if line.starts_with('-') || line.starts_with('+') {
            return original;
        }
        if line.starts_with(' ') || line.is_empty() {
            original += 1;
        }
    }
    start
}

/// One diagnostic in the short `path:line:col: level: message` form that clippy (through cargo's
/// JSON `rendered` field) and `typos --format brief` print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Diag {
    pub path: String,
    pub line: u32,
    /// Includes the level and, from JSON, the lint and the help notes:
    /// `warning: unused variable: x (unused_variables)` or `error[E0425]: cannot find value`.
    pub message: String,
    pub is_error: bool,
}

/// Every short-form diagnostic in `output`, deduplicated by (path, line, message). A line is either
/// one of cargo's JSON messages, whose `rendered` text is read, whose lint name is appended and
/// whose help notes follow, or plain `path:line:col: warning|error...` text. Lines without a file
/// prefix are ignored.
pub(crate) fn short_diags(output: &str, root: &str) -> Vec<Diag> {
    let mut diags: Vec<Diag> = Vec::new();
    for line in output.lines() {
        let diag = match json_message(line) {
            Some(message) => message.rendered.lines().next().and_then(|text| {
                short_line(text, root)
                    .map(|diag| with_help(with_lint(diag, message.code.as_deref()), &message.help))
            }),
            None => short_line(line, root),
        };
        let Some(diag) = diag else {
            continue;
        };
        let duplicate = diags
            .iter()
            .any(|d| d.path == diag.path && d.line == diag.line && d.message == diag.message);
        if !duplicate {
            diags.push(diag);
        }
    }
    diags
}

/// What people read of clippy's JSON output: each diagnostic's `rendered` text, and every line
/// that is not JSON (cargo's own messages). Other JSON messages, such as artifacts, are dropped.
pub(crate) fn clippy_render(output: &str) -> String {
    let mut text = String::new();
    for line in output.lines() {
        if line.starts_with('{') {
            if let Some(message) = json_message(line) {
                text.push_str(message.rendered.trim_end());
                text.push('\n');
            }
        } else {
            text.push_str(line);
            text.push('\n');
        }
    }
    text
}

/// The parts of a cargo `compiler-message` that xtask reads.
struct JsonMessage {
    rendered: String,
    /// The lint or error code: `clippy::unwrap_used`, `unused_variables`, `E0425`.
    code: Option<String>,
    /// The notes under a ban's diagnostic, in the order clippy printed them; empty for other lints.
    help: Vec<String>,
}

/// The diagnostic in one line of cargo's JSON output, or `None` for any other line.
fn json_message(line: &str) -> Option<JsonMessage> {
    if !line.starts_with('{') {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("reason")?.as_str()? != "compiler-message" {
        return None;
    }
    let message = value.get("message")?;
    let code = message
        .get("code")
        .and_then(|code| code.get("code"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let help = match code.as_deref() {
        Some(code) if BANS.contains(&code) => help_notes(message),
        _ => Vec::new(),
    };
    Some(JsonMessage {
        rendered: message.get("rendered")?.as_str()?.to_owned(),
        code,
        help,
    })
}

/// The clippy lints whose notes are printed: a ban in `clippy.toml` gives its reason as a note or
/// help, and its replacement as a suggestion. Every other lint prints its rendered text alone.
const BANS: [&str; 2] = ["clippy::disallowed_methods", "clippy::disallowed_types"];

/// Appends ` (code)` to the message, unless the text already shows it as `error[code]`.
fn with_lint(mut diag: Diag, code: Option<&str>) -> Diag {
    if let Some(code) = code
        && !diag.message.contains(&format!("[{code}]"))
    {
        diag.message = format!("{} ({code})", diag.message);
    }
    diag
}

/// The help notes under a ban, one piece per kept child: the child's message, then `use X` (with
/// the replacement in backticks) when a span suggests replacement `X`. A child is kept when it is
/// a `help`, or a `note` with no span, and it is neither boilerplate nor a note that asks for an
/// `#[allow]`. A note that points at code (`type is ...`) stays out. A ban prints its reason as a
/// `note` when it has no replacement, and as a `help` that carries the replacement when it has one.
fn help_notes(message: &serde_json::Value) -> Vec<String> {
    let mut notes = Vec::new();
    let children = message
        .get("children")
        .and_then(serde_json::Value::as_array);
    for child in children.into_iter().flatten() {
        let text = child.get("message").and_then(serde_json::Value::as_str);
        let level = child.get("level").and_then(serde_json::Value::as_str);
        let spans = child
            .get("spans")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let (Some(text), Some(level)) = (text, level) else {
            continue;
        };
        let text = one_line(text);
        let kept = (level == "help" || (level == "note" && spans.is_empty()))
            && !text.is_empty()
            && !is_boilerplate(&text)
            && !asks_for_allow(&text);
        if !kept {
            continue;
        }
        let replacement = spans
            .iter()
            .filter_map(|span| span.get("suggested_replacement")?.as_str())
            .map(one_line)
            .find(|replacement| !replacement.is_empty());
        notes.push(text);
        if let Some(replacement) = replacement {
            notes.push(format!("use `{replacement}`"));
        }
    }
    notes
}

/// The notes that clippy and rustc print under a finding and that say nothing about why it fired:
/// the lint's docs link, the source of a lint level, the default level of a rustc lint, and the
/// level that `-D warnings` gives a lint.
fn is_boilerplate(text: &str) -> bool {
    text.starts_with("for further information visit ")
        || text.starts_with("requested on the command line")
        || text.ends_with(" on by default")
        || text.contains("implied by `-D warnings`")
}

/// A note that tells the reader to silence the lint with `#[allow]`. AGENTS.md bans that attribute,
/// so no note that names one is printed.
fn asks_for_allow(text: &str) -> bool {
    text.contains("[allow")
}

/// `text` with every run of whitespace, a newline included, as one space: a note stays on the
/// finding's line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Appends the help notes to a finding's message: `: `, then the notes joined by `; `.
fn with_help(mut diag: Diag, help: &[String]) -> Diag {
    if !help.is_empty() {
        diag.message = format!("{}: {}", diag.message, help.join("; "));
    }
    diag
}

fn short_line(line: &str, root: &str) -> Option<Diag> {
    let at = [": warning", ": error"]
        .iter()
        .filter_map(|marker| line.find(*marker))
        .min()?;
    let is_error = line[at..].starts_with(": error");
    let mut parts = line[..at].rsplitn(3, ':');
    let column = parts.next()?;
    let row = parts.next()?;
    let path = parts.next()?;
    let line_no: u32 = row.parse().ok()?;
    column.parse::<u32>().ok()?;
    if path.is_empty() {
        return None;
    }
    Some(Diag {
        path: rel_path(path, root),
        line: line_no,
        message: line[at + 2..].to_string(),
        is_error,
    })
}

/// Findings for `cargo fmt --check` output: one per file it would change, fixed by
/// `cargo fmt --all`.
pub(crate) fn fmt_findings(output: &str, ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    fmt_diffs(output, &root_str(ctx))
        .into_iter()
        .map(|(path, line)| Finding {
            violation: Violation {
                path,
                line,
                message: "rustfmt would reformat this code".into(),
                fix: "run `cargo fmt --all`".into(),
            },
            is_error: true,
        })
        .collect()
}

/// Findings for clippy's output (see `short_diags`). Each fix line is the command that shows
/// the full diagnostic, built from the args that ran (see `clippy_repro`).
pub(crate) fn clippy_findings(output: &str, ctx: &Context, cmd: &Cmd) -> Vec<Finding> {
    // With no members, from a failed `cargo metadata`, the selectors that ran stay.
    let members = ctx.members.as_deref().unwrap_or_default();
    short_diags(output, &root_str(ctx))
        .into_iter()
        .map(|diag| {
            let fix = clippy_fix(cmd, &diag.path, members);
            Finding {
                violation: Violation {
                    path: diag.path,
                    line: diag.line,
                    message: diag.message,
                    fix,
                },
                is_error: diag.is_error,
            }
        })
        .collect()
}

fn root_str(ctx: &Context) -> String {
    ctx.root.to_string_lossy().into_owned()
}

/// The fix line for a clippy finding in `path`: the command that shows the full diagnostic.
fn clippy_fix(cmd: &Cmd, path: &str, members: &[Member]) -> String {
    format!(
        "`{}` shows the full diagnostic and clippy's suggested change",
        clippy_repro(cmd, path, members)
    )
}

/// The clippy command narrowed to the package that owns `path`, built from the args that ran.
/// Drops `--workspace`, every `-p <name>` and `--message-format=...`, then inserts `-p <owner>`
/// after `clippy`. With no owner, the original selectors stay.
fn clippy_repro(cmd: &Cmd, path: &str, members: &[Member]) -> Cmd {
    let owner = package_for(path, members);
    let mut args = Vec::new();
    let mut rest = cmd.args.iter();
    args.extend(rest.next().cloned());
    if let Some(owner) = owner {
        args.extend(["-p".to_owned(), owner.name.clone()]);
    }
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            // Everything after a literal `--` is passed to clippy's lints, copied verbatim.
            "--" => {
                args.push(arg.clone());
                args.extend(rest.by_ref().cloned());
            }
            format if format.starts_with("--message-format=") => {}
            "--workspace" if owner.is_some() => {}
            "-p" if owner.is_some() => {
                rest.next();
            }
            _ => args.push(arg.clone()),
        }
    }
    Cmd {
        program: cmd.program.clone(),
        args,
    }
}

#[cfg(test)]
mod tests;
