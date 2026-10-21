//! The `expect-allowlist` check: every `#[expect(..)]` and `#![expect(..)]` in the repo's Rust
//! files has its own line in `xtask/expect-allowlist.txt`, every line there has its own site,
//! and no file lowers a lint with `allow` or `warn` in any form. `deny` and `forbid` only
//! raise a lint, so they pass. The forms that would hide an attribute from this check fail too:
//! an attribute in a macro body whose name or lint is a metavariable, a `#` in a macro body with
//! no brackets after it, and a `#[path]` or `include!` that makes rustc compile a file this
//! check does not read (see `sites`).
//!
//! The files are every `.rs` file (in any case) that `changes::repo_files` lists: tracked files,
//! and untracked files that `.gitignore` does not exclude, so a new file is caught before anyone
//! adds it, as `file-size` does. Each is read and parsed once, in this process (`sites`). The
//! check counts the files it read, those syn cannot parse included.
//!
//! A line is `<path> <item> <lint>`; `#` starts a comment and blank lines are skipped. A site and
//! a line match on all three fields, and N identical sites need N identical lines. The order of
//! the lines is not checked. The header of the allowlist says how `<item>` is named.

mod fix;
mod sites;

pub(crate) use fix::Fix;

use crate::changes;
use crate::checks::{self, Failure, Inspection, Noun, StaticCheck, line_at};
use crate::violation::Violation;
use crate::workspace::Context;
use fix::Level;
use sites::{Kind, Site};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The allowlist, repo-relative.
pub(super) const ALLOWLIST: &str = "xtask/expect-allowlist.txt";

/// Fails on an unlisted `expect`, a stale allowlist line, any `allow` or `warn`, an attribute it
/// cannot read, a `#[path]` or `include!` of a file it does not read, a malformed line and a
/// Rust file syn cannot parse.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "expect-allowlist",
    counts: Noun {
        one: "Rust file",
        many: "Rust files",
    },
    place: || format!("in the repo, matched against {ALLOWLIST}"),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// One line of the allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    key: Key,
    /// Its 1-based line in the allowlist.
    line: u32,
}

/// What a site and a line match on: `(path, item, lint)`.
type Key = (String, String, String);

/// One `expect` site, kept for matching.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Expect {
    key: Key,
    line: u32,
    /// True for `#![expect]`.
    inner: bool,
}

/// Lists the Rust files, finds their sites and matches them against the allowlist.
fn run(ctx: &Context) -> Inspection {
    let files = match changes::repo_files(&ctx.root) {
        Ok(files) => files,
        Err(error) => return checks::unlisted(CHECK.name, &error),
    };
    let mut violations = Vec::new();
    let mut expects: Vec<Expect> = Vec::new();
    // Files whose sites are unknown: a line naming one is not called stale.
    let mut unknown = BTreeSet::new();
    let mut read = 0;
    let listed: BTreeSet<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|file| is_rust(file))
        .collect();
    for file in listed.iter().copied() {
        let text = match checks::read_repo_file(&ctx.root, file) {
            Ok(Some(text)) => text,
            Ok(None) => continue,
            Err(violation) => {
                unknown.insert(file.to_owned());
                violations.push(violation);
                continue;
            }
        };
        read += 1;
        match sites::find(&text) {
            Ok(found) => {
                for site in found {
                    violations.extend(site_violation(file, &site, &listed, &mut expects));
                }
            }
            Err(error) => {
                unknown.insert(file.to_owned());
                violations.push(unparsable(file, &error));
            }
        }
    }
    let mut failures = Vec::new();
    match checks::read_repo_file(&ctx.root, ALLOWLIST) {
        Ok(Some(text)) => {
            let (entries, malformed) = parse(&text);
            violations.extend(malformed);
            violations.extend(unmatched(&expects, &entries, &unknown));
        }
        Ok(None) => failures.push(Failure {
            message: format!("{ALLOWLIST} is missing, so no #[expect] site was matched against it"),
            fix: Fix::MissingAllowlist.into(),
        }),
        Err(violation) => violations.push(violation),
    }
    Inspection {
        inspected: read,
        violations,
        failures,
        notes: Vec::new(),
        stopped: read == 0 && !unknown.is_empty(),
    }
}

/// True for a `.rs` file, in any case.
fn is_rust(file: &str) -> bool {
    Path::new(file)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Keeps an `expect` site in `expects` for matching; every other site is a violation at once,
/// except a `#[path]` or `include!` of a file in `listed`, the `.rs` files the check reads.
fn site_violation(
    file: &str,
    site: &Site,
    listed: &BTreeSet<&str>,
    expects: &mut Vec<Expect>,
) -> Option<Violation> {
    let (message, fix) = match &site.kind {
        Kind::Expect { lint, inner } => {
            expects.push(Expect {
                key: (file.to_owned(), site.item.clone(), lint.clone()),
                line: site.line,
                inner: *inner,
            });
            return None;
        }
        Kind::Lower { level, lint, inner } => {
            let bang = if *inner { "!" } else { "" };
            let what = match level {
                Level::Allow => "silences a lint",
                Level::Warn => {
                    "turns a denied lint into a warning, which a local cargo xtask check passes"
                }
            };
            let fix = Fix::Lower {
                level: *level,
                path: file.to_owned(),
                item: site.item.clone(),
                lint: lint.clone(),
                inner: *inner,
            };
            let message = format!(
                "#{bang}[{level}({lint})] {what}; only #[expect] with a reason may lower a lint"
            );
            (message, fix)
        }
        Kind::EmptyLower { level } => (
            format!(
                "{} {level} attribute that names no lint",
                if *level == Level::Allow { "an" } else { "a" }
            ),
            Fix::EmptyLower { level: *level },
        ),
        Kind::Metavariable => (
            "an attribute in a macro body takes its name or a lint from a metavariable, so \
             expect-allowlist cannot see whether it is an expect, allow or warn"
                .to_owned(),
            Fix::MetavariableAttribute,
        ),
        Kind::HashWithoutBrackets => (
            "a `#` in a macro body has no brackets after it, so the caller supplies the \
             attribute and expect-allowlist cannot read it"
                .to_owned(),
            Fix::HashWithoutBrackets,
        ),
        Kind::Path { value, resolve } => (
            path_message(file, value.as_deref(), *resolve, listed)?,
            Fix::Path,
        ),
        Kind::Include { target } => (
            include_message(file, target.as_deref(), listed)?,
            Fix::Include,
        ),
    };
    Some(checks::violation(file, site.line, message, fix))
}

/// Why a `#[path]` with `value` in `file` fails, or `None` when it names a file in `listed`.
fn path_message(
    file: &str,
    value: Option<&str>,
    resolve: bool,
    listed: &BTreeSet<&str>,
) -> Option<String> {
    let Some(value) = value else {
        return Some(
            "#[path] takes a value that is not a string literal, so expect-allowlist cannot tell \
             which file rustc compiles"
                .to_owned(),
        );
    };
    if !resolve {
        return Some(format!(
            "#[path = \"{value}\"] is not on a module at the top level of the file, so \
             expect-allowlist cannot tell which file rustc compiles"
        ));
    }
    unread(file, value, listed)
        .map(|target| format!("#[path = \"{value}\"] names {target}, {NOT_READ}"))
}

/// Why an `include!` of `target` in `file` fails, or `None` when it names a file in `listed`.
fn include_message(file: &str, target: Option<&str>, listed: &BTreeSet<&str>) -> Option<String> {
    let Some(target) = target else {
        return Some(
            "include! takes an argument that is not a string literal, so expect-allowlist cannot \
             tell which file rustc compiles"
                .to_owned(),
        );
    };
    unread(file, target, listed)
        .map(|resolved| format!("include!(\"{target}\") compiles {resolved}, {NOT_READ}"))
}

/// The end of the message for a `#[path]` or `include!` of a file the check does not read.
const NOT_READ: &str = "which is not a .rs file git lists, so expect-allowlist does not read it";

/// `target`, relative to the folder of `file`, as the check names it when it is not one of
/// `listed`: repo-relative when it stays inside the repo, else as written. `None` when the
/// check reads it.
fn unread(file: &str, target: &str, listed: &BTreeSet<&str>) -> Option<String> {
    match resolve(file, target) {
        Some(path) if listed.contains(path.as_str()) => None,
        Some(path) => Some(format!("`{path}`")),
        None => Some(format!("`{target}`, outside the repo")),
    }
}

/// `target` joined to the folder of `file` and made repo-relative, with `.` and `..` folded.
/// `None` for an absolute path, a Windows path, or one that leaves the repo.
fn resolve(file: &str, target: &str) -> Option<String> {
    if target.starts_with('/') || target.contains('\\') || target.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// The violation for a Rust file syn cannot parse, at the error's line.
fn unparsable(file: &str, error: &syn::Error) -> Violation {
    let line = u32::try_from(error.span().start().line)
        .unwrap_or(u32::MAX)
        .max(1);
    checks::violation(
        file,
        line,
        format!("`{file}` is not valid Rust: {error}"),
        Fix::Unparsable,
    )
}

/// The entries of the allowlist `text`, and a violation for each line that is not three fields.
fn parse(text: &str) -> (Vec<Entry>, Vec<Violation>) {
    let mut entries = Vec::new();
    let mut violations = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let entry = raw.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        let line = line_at(index);
        let fields: Vec<&str> = entry.split_whitespace().collect();
        if let [path, item, lint] = fields[..] {
            entries.push(Entry {
                key: (path.to_owned(), item.to_owned(), lint.to_owned()),
                line,
            });
        } else {
            violations.push(checks::violation(
                ALLOWLIST,
                line,
                format!(
                    "this line has {} fields; a line is \"<path> <item> <lint>\"",
                    fields.len()
                ),
                Fix::Malformed,
            ));
        }
    }
    (entries, violations)
}

/// Sites and lines left over once each site has taken one identical line, in order: the later
/// sites of a key with too few lines, and the later lines of a key with too few sites. A line for
/// a file whose sites are unknown (`unknown`) is never called stale.
fn unmatched(expects: &[Expect], entries: &[Entry], unknown: &BTreeSet<String>) -> Vec<Violation> {
    let mut sites: BTreeMap<&Key, Vec<&Expect>> = BTreeMap::new();
    for expect in expects {
        sites.entry(&expect.key).or_default().push(expect);
    }
    let mut lines: BTreeMap<&Key, Vec<u32>> = BTreeMap::new();
    for entry in entries {
        lines.entry(&entry.key).or_default().push(entry.line);
    }
    let mut found = Vec::new();
    for (key, at) in &sites {
        let listed = lines.get(key).map_or(0, Vec::len);
        for site in at.iter().skip(listed) {
            found.push(unlisted(site, listed, at.len()));
        }
    }
    for (key, at) in &lines {
        if unknown.contains(&key.0) {
            continue;
        }
        let present = sites.get(key).map_or(0, Vec::len);
        for line in at.iter().skip(present) {
            found.push(stale(key, *line, present, at.len()));
        }
    }
    found.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    found
}

/// The violation for the `expect` site `site` with no line of its own: `listed` lines for
/// `count` identical sites.
fn unlisted(site: &Expect, listed: usize, count: usize) -> Violation {
    let (path, item, lint) = &site.key;
    let bang = if site.inner { "!" } else { "" };
    let place = place(item);
    let message = if listed == 0 {
        format!("#{bang}[expect({lint})] {place} is not in {ALLOWLIST}")
    } else {
        format!(
            "#{bang}[expect({lint})] {place} is in {ALLOWLIST} {}, but this file has {count} \
             such sites",
            times(listed)
        )
    };
    let fix = Fix::Unlisted {
        path: path.clone(),
        item: item.clone(),
        lint: lint.clone(),
    };
    checks::violation(path, site.line, message, fix)
}

/// Where a site of `item` is, for a message: in `f`, or at the top of the file for `-`.
fn place(item: &str) -> String {
    if item == sites::FILE_TOP {
        "at the top of the file".to_owned()
    } else {
        format!("in `{item}`")
    }
}

/// The violation for the allowlist line `line` of `key` with no site of its own: `present`
/// sites for `count` identical lines.
fn stale(key: &Key, line: u32, present: usize, count: usize) -> Violation {
    let (path, item, lint) = key;
    let message = if present == 0 {
        let bang = if item == sites::FILE_TOP { "!" } else { "" };
        format!(
            "stale entry: {path} has no #{bang}[expect({lint})] {}",
            place(item)
        )
    } else {
        format!(
            "stale entry: \"{path} {item} {lint}\" is listed {}, but {path} has {present} \
             such {}",
            times(count),
            if present == 1 { "site" } else { "sites" }
        )
    };
    checks::violation(ALLOWLIST, line, message, Fix::Stale { line })
}

/// `once`, `twice`, `N times`.
fn times(n: usize) -> String {
    match n {
        1 => "once".to_owned(),
        2 => "twice".to_owned(),
        _ => format!("{n} times"),
    }
}

#[cfg(test)]
mod tests;
