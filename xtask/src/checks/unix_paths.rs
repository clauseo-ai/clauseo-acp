//! The `unix-paths` check: a test may not hold a Unix absolute path as a string literal, since on
//! Windows `"/opt/homebrew/bin/npx"` is not absolute, and a test that hands one to `Path`,
//! `is_absolute` or the file system passes on Unix and fails only on the Windows gate.
//!
//! It flags a string literal, raw or not, whose value starts with `/opt/`, `/usr/`, `/tools/`,
//! `/Users/` or `/home/`, in a `.rs` file under `crates/<name>/tests/`, or under
//! `crates/<name>/src/` with `tests` in its file name or a `tests` folder on its path. Literals in
//! comments and doc comments are not literals to the lexer (`scan.rs`). It only counts, in the
//! note it prints, literals that start with `/tmp/`, `/var/` or `/etc/`, and the literals it would
//! flag in the in-file `#[cfg(test)]` code of other source files.
//!
//! It exempts
//! - code a cfg keeps out of every Windows build (`unix`, `not(windows)`, a `target_os` other than
//!   `windows`, and `all` or `any` of them), whether the cfg is on the item, on a module around
//!   it, or on the `mod name;` line that pulls its file in (`modules.rs`);
//! - a literal inside `testing::abs(..)`, under any path, or a bare `abs(..)`;
//! - a file with `// unix-paths-ok: <reason>` in its leading comment block, and a line with it
//!   on the line itself or the line above; the marker needs a reason;
//! - the protocol crate's snapshots and corpus, and every `testing.rs`.
//!
//! What main held when the check came in is recorded in `xtask/unix-paths-baseline.txt`, one
//! `<path> "<literal>"` line per literal, as the expect-allowlist records its sites: N identical
//! literals in a file need N identical lines, and the line numbers are not recorded. A literal
//! with no line of its own fails, and so does a line with no literal left (stale), at its line.
//!
//! The files come from `changes::repo_files`, as file-size's do. It counts the test files it
//! read.

mod fix;
mod modules;
mod scan;

pub(crate) use fix::Fix;

use crate::changes;
use crate::checks::core_io::cfgs::is_rust_file;
use crate::checks::{self, Inspection, Noun, StaticCheck, line_at};
use crate::violation::Violation;
use crate::workspace::Context;
use std::collections::{BTreeMap, BTreeSet};

/// The baseline, repo-relative.
pub(super) const BASELINE: &str = "xtask/unix-paths-baseline.txt";
/// The folder whose crates the check reads.
const CRATES: &str = "crates";
/// The prefixes a literal fails on.
const FLAGGED: [&str; 5] = ["/opt/", "/usr/", "/tools/", "/Users/", "/home/"];
/// The prefixes the note counts, which never fail.
const REPORTED: [&str; 3] = ["/tmp/", "/var/", "/etc/"];
/// The folders whose files hold recorded data, not tests.
const DATA_DIRS: [&str; 2] = [
    "crates/protocol/tests/corpus",
    "crates/protocol/tests/snapshots",
];
/// The file name of the helper that makes a path absolute on every OS.
const HELPER: &str = "testing.rs";
/// The comment that exempts a file or a line.
pub(super) const MARKER: &str = "// unix-paths-ok:";

/// Fails on a Unix absolute path in a test that the baseline does not list, on a stale baseline
/// line, on a marker with no reason and on a test file that does not lex.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "unix-paths",
    counts: Noun {
        one: "test file",
        many: "test files",
    },
    place: || format!("under {CRATES}/*/tests and in the test files under {CRATES}/*/src"),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// Where a file stands to the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// A test file: its literals are flagged.
    Test,
    /// A snapshot, the corpus or `testing.rs`: its literals are counted as exempt.
    Data,
    /// Any other source file: only its `#[cfg(test)]` code is counted.
    Source,
}

/// What the note counts.
#[derive(Debug, Default)]
struct Tally {
    unix: usize,
    abs: usize,
    file_marker: usize,
    line_marker: usize,
    data: usize,
    baselined: usize,
    in_cfg_test: usize,
    reported: [usize; 3],
}

/// One flagged literal that nothing exempts.
#[derive(Debug)]
struct Hit {
    path: String,
    line: u32,
    value: String,
}

fn run(ctx: &Context) -> Inspection {
    let files = match changes::repo_files(&ctx.root) {
        Ok(files) => files,
        Err(error) => return checks::unlisted(CHECK.name, &error),
    };
    let mut violations = Vec::new();
    let mut texts = BTreeMap::new();
    let mut scans = BTreeMap::new();
    let mut unknown = BTreeSet::new();
    let prefixes: Vec<&str> = FLAGGED.iter().chain(&REPORTED).copied().collect();
    for file in files.iter().filter(|file| is_crate_source(file)) {
        let test = scope(file) == Scope::Test;
        let text = match checks::read_repo_file(&ctx.root, file) {
            Ok(Some(text)) => text,
            Ok(None) => continue,
            Err(violation) => {
                if test {
                    unknown.insert(file.clone());
                    violations.push(violation);
                }
                continue;
            }
        };
        match scan::scan(&text, &prefixes) {
            Ok(found) => {
                scans.insert(file.clone(), found);
                texts.insert(file.clone(), text);
            }
            Err(error) if test => {
                unknown.insert(file.clone());
                let line = checks::core_io::cfgs::line_of(error.span().start().line);
                violations.push(checks::violation(
                    file,
                    line,
                    format!("`{file}` does not lex as Rust: {error}"),
                    Fix::NotLexed,
                ));
            }
            Err(_) => {}
        }
    }
    let unix_files = modules::unix_files(&scans);
    let mut tally = Tally::default();
    let mut hits = Vec::new();
    let mut read = unknown.len();
    for (file, found) in &scans {
        let text = &texts[file];
        let scope = scope(file);
        if scope == Scope::Test {
            read += 1;
            violations.extend(bare_markers(file, text));
        }
        let unix_file = unix_files.contains(file);
        let file_marker = file_marked(text);
        let lines: Vec<&str> = text.lines().collect();
        for literal in &found.literals {
            if let Some(slot) = REPORTED
                .iter()
                .position(|prefix| literal.value.starts_with(prefix))
            {
                if scope == Scope::Test {
                    tally.reported[slot] += 1;
                }
                continue;
            }
            let unix = literal.unix || unix_file;
            let counter = match scope {
                Scope::Data => &mut tally.data,
                Scope::Source if literal.test && !unix && !literal.abs => &mut tally.in_cfg_test,
                Scope::Source => continue,
                Scope::Test if unix => &mut tally.unix,
                Scope::Test if literal.abs => &mut tally.abs,
                Scope::Test if file_marker => &mut tally.file_marker,
                Scope::Test if line_marked(&lines, literal.line) => &mut tally.line_marker,
                Scope::Test => {
                    hits.push(Hit {
                        path: file.clone(),
                        line: literal.line,
                        value: literal.value.clone(),
                    });
                    continue;
                }
            };
            *counter += 1;
        }
    }
    match checks::read_repo_file(&ctx.root, BASELINE) {
        Ok(text) => {
            let (entries, malformed) = parse(text.as_deref().unwrap_or_default());
            violations.extend(malformed);
            let (unlisted, stale) = unmatched(&hits, &entries, &unknown);
            tally.baselined = hits.len() - unlisted.len();
            violations.extend(unlisted);
            violations.extend(stale);
        }
        Err(violation) => violations.push(violation),
    }
    Inspection {
        inspected: read,
        violations,
        failures: Vec::new(),
        notes: vec![note(&tally)],
        stopped: read == unknown.len() && !unknown.is_empty(),
    }
}

/// True for a `.rs` file under `crates/<name>/`.
fn is_crate_source(file: &str) -> bool {
    is_rust_file(file) && file.split('/').count() > 2 && file.starts_with(&format!("{CRATES}/"))
}

/// Where `file`, a `.rs` file under `crates/<name>/`, stands to the check.
fn scope(file: &str) -> Scope {
    let parts: Vec<&str> = file.split('/').collect();
    let name = parts.last().copied().unwrap_or_default();
    if name == HELPER
        || DATA_DIRS
            .iter()
            .any(|dir| crate::workspace::is_under(file, dir))
    {
        return Scope::Data;
    }
    match parts.get(2..) {
        Some(["tests", ..]) => Scope::Test,
        Some(["src", inner @ ..]) => {
            let folders = &inner[..inner.len().saturating_sub(1)];
            if name.contains("tests") || folders.contains(&"tests") {
                Scope::Test
            } else {
                Scope::Source
            }
        }
        _ => Scope::Source,
    }
}

/// The reason after the marker on `line`, when the marker is there.
fn marker_reason(line: &str) -> Option<&str> {
    line.find(MARKER).map(|at| line[at + MARKER.len()..].trim())
}

/// True when the marker, with a reason, is in the leading comment block of `text`.
fn file_marked(text: &str) -> bool {
    text.lines()
        .take_while(|line| line.trim().is_empty() || line.trim_start().starts_with("//"))
        .any(|line| marker_reason(line).is_some_and(|reason| !reason.is_empty()))
}

/// True when the marker, with a reason, is on `line` (1-based) of `lines` or the line above.
fn line_marked(lines: &[&str], line: u32) -> bool {
    let index = usize::try_from(line)
        .unwrap_or(usize::MAX)
        .saturating_sub(1);
    [Some(index), index.checked_sub(1)]
        .into_iter()
        .flatten()
        .filter_map(|at| lines.get(at))
        .any(|text| marker_reason(text).is_some_and(|reason| !reason.is_empty()))
}

/// A violation for each marker in `text` with no reason after it.
fn bare_markers(file: &str, text: &str) -> Vec<Violation> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| marker_reason(line) == Some(""))
        .map(|(index, _)| {
            checks::violation(
                file,
                line_at(index),
                "this unix-paths-ok marker gives no reason, so it exempts nothing",
                Fix::GiveReason,
            )
        })
        .collect()
}

/// One baseline line: a file and a literal, quoted as Rust writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    key: (String, String),
    line: u32,
}

/// The entries of the baseline `text`, and a violation for each line that is not
/// `<path> "<literal>"`.
fn parse(text: &str) -> (Vec<Entry>, Vec<Violation>) {
    let mut entries = Vec::new();
    let mut violations = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let entry = raw.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        let line = line_at(index);
        let parsed = entry.split_once(' ').and_then(|(path, quoted)| {
            let quoted = quoted.trim();
            let value = syn::parse_str::<syn::LitStr>(quoted).ok()?.value();
            Some((path.to_owned(), value))
        });
        match parsed {
            Some(key) => entries.push(Entry { key, line }),
            None => violations.push(checks::violation(
                BASELINE,
                line,
                "this line is not \"<path> \\\"<literal>\\\"\"",
                Fix::Malformed,
            )),
        }
    }
    (entries, violations)
}

/// The hits with no baseline line of their own, and the baseline lines with no hit of their own,
/// matched by file and literal, each sorted. A line for a file in `unknown`, which could not be
/// read or lexed, is never stale.
fn unmatched(
    hits: &[Hit],
    entries: &[Entry],
    unknown: &BTreeSet<String>,
) -> (Vec<Violation>, Vec<Violation>) {
    let mut found: BTreeMap<(String, String), Vec<&Hit>> = BTreeMap::new();
    for hit in hits {
        found
            .entry((hit.path.clone(), hit.value.clone()))
            .or_default()
            .push(hit);
    }
    let mut listed: BTreeMap<&(String, String), Vec<u32>> = BTreeMap::new();
    for entry in entries {
        listed.entry(&entry.key).or_default().push(entry.line);
    }
    let mut unlisted = Vec::new();
    for (key, at) in &found {
        let count = listed.get(key).map_or(0, Vec::len);
        for hit in at.iter().skip(count) {
            unlisted.push(new_hit(hit, count, at.len()));
        }
    }
    let mut stale = Vec::new();
    for (key, at) in &listed {
        if unknown.contains(&key.0) {
            continue;
        }
        let present = found.get(*key).map_or(0, Vec::len);
        for line in at.iter().skip(present) {
            stale.push(stale_line(key, *line, present, at.len()));
        }
    }
    unlisted.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    stale.sort_by_key(|violation| violation.line);
    (unlisted, stale)
}

/// The violation for `hit`, which has no baseline line: `listed` lines for `count` such literals.
fn new_hit(hit: &Hit, listed: usize, count: usize) -> Violation {
    let mut message = format!(
        "{:?} is a Unix absolute path, which is not absolute on Windows",
        hit.value
    );
    if listed > 0 {
        let times = if listed == 1 { "time" } else { "times" };
        message =
            format!("{message}; {BASELINE} lists it {listed} {times}, but this file has {count}");
    }
    checks::violation(&hit.path, hit.line, message, Fix::UseAbs)
}

/// The violation for the baseline line `line`, of `key`, with no literal of its own: `present`
/// literals for `count` such lines.
fn stale_line(key: &(String, String), line: u32, present: usize, count: usize) -> Violation {
    let (path, value) = key;
    let message = if present == 0 {
        format!("stale entry: {path} no longer holds {value:?}")
    } else {
        format!("stale entry: {path} {value:?} is listed {count} times, but {path} has {present}")
    };
    checks::violation(BASELINE, line, message, Fix::Stale { line })
}

/// The one note: what the baseline holds, what each exemption took and what is only counted.
fn note(tally: &Tally) -> String {
    let [tmp, var, etc] = tally.reported;
    format!(
        "unix-paths: {} baselined in {BASELINE}; exempt: {} Unix-only by cfg, {} in abs(), {} by \
         file marker, {} by line marker, {} in snapshots, corpus or testing.rs; counted only: {tmp} \
         /tmp/, {var} /var/ and {etc} /etc/ literals in tests, {} flagged prefixes in in-file \
         #[cfg(test)] code",
        tally.baselined,
        tally.unix,
        tally.abs,
        tally.file_marker,
        tally.line_marker,
        tally.data,
        tally.in_cfg_test,
    )
}

#[cfg(test)]
mod tests;
