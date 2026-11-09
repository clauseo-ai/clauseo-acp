//! The `log-literals` check: every `warn!` and `error!` in the Windows sandbox's code
//! (`crates/sandbox/src/windows/`, but not `windows/setup/`, the setup binary's) has a literal
//! message with no format argument, neither `{name}` in the literal nor an argument after it.
//!
//! The outer runner writes those two levels to `runner.log` (`windows/runner/log_file.rs`), whose
//! formatter keeps the message as it is: it can't tell a literal from a formatted message, as
//! `tracing` hands both over as `fmt::Arguments` behind `&dyn Debug`. A formatted message could
//! carry a matter's name or a file name into the file, so this check keeps them out at the source;
//! a value goes in a field, which the formatter drops unless its name is one it keeps.
//!
//! It reads each `.rs` file there that `git ls-files` lists (tracked, or untracked and not
//! ignored), except tests: a file named `tests.rs` or ending in `_tests.rs`, or under a `tests`
//! folder. It parses each with syn and finds the macros by their last path segment, so
//! `tracing::warn!` and an imported `warn!` both count. It counts the files it read.

mod fix;
pub(crate) use fix::Fix;

use crate::changes;
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

/// The folder whose files the check reads, repo-relative.
const DIR: &str = "crates/sandbox/src/windows/";
/// The folder under `DIR` it skips: the setup binary's, which never writes `runner.log`.
const SKIPPED: &str = "crates/sandbox/src/windows/setup/";
/// The macros whose message must be a literal: the levels `runner.log` keeps.
const MACROS: [&str; 2] = ["warn", "error"];

/// Fails on a `warn!` or `error!` with a formatted message in the Windows sandbox's code.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "log-literals",
    counts: Noun {
        one: "Rust file",
        many: "Rust files",
    },
    place: || format!("in {DIR}, except {SKIPPED} and tests"),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

fn run(ctx: &Context) -> Inspection {
    let files = match changes::repo_files(&ctx.root) {
        Ok(files) => files,
        Err(error) => return checks::unlisted(CHECK.name, &error),
    };
    let mut found = Inspection::default();
    for file in files.iter().filter(|file| is_read(file)) {
        match checks::read_repo_file(&ctx.root, file) {
            Ok(Some(text)) => {
                found.inspected += 1;
                found.violations.extend(inspect(file, &text));
            }
            Ok(None) => {}
            Err(violation) => found.violations.push(violation),
        }
    }
    found
}

/// True for a `.rs` file under `DIR`, not under `SKIPPED`, and not a test file.
fn is_read(file: &str) -> bool {
    let Some(rest) = file.strip_prefix(DIR) else {
        return false;
    };
    let name = rest.rsplit('/').next().unwrap_or(rest);
    let test = name == "tests.rs"
        || name.ends_with("_tests.rs")
        || rest.split('/').any(|part| part == "tests");
    name.ends_with(".rs") && !file.starts_with(SKIPPED) && !test
}

/// The violations of the file `path` holding `text`.
fn inspect(path: &str, text: &str) -> Vec<Violation> {
    let parsed = match syn::parse_file(text) {
        Ok(parsed) => parsed,
        Err(error) => {
            let line = u32::try_from(error.span().start().line).unwrap_or(1).max(1);
            let message = format!("syn cannot parse this file: {error}");
            return vec![checks::violation(path, line, message, Fix::Unparsable)];
        }
    };
    let mut calls = Calls {
        path,
        found: Vec::new(),
    };
    calls.visit_file(&parsed);
    calls.found
}

/// The `warn!` and `error!` calls of one file, and the violations among them.
struct Calls<'a> {
    path: &'a str,
    found: Vec<Violation>,
}

impl<'ast> Visit<'ast> for Calls<'_> {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let name = mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        if let Some(name) = name.filter(|name| MACROS.contains(&name.as_str()))
            && formatted(&mac.tokens)
        {
            let start = mac.path.segments.first().map_or_else(
                || mac.bang_token.span.start(),
                |segment| segment.ident.span().start(),
            );
            let line = u32::try_from(start.line).unwrap_or(1).max(1);
            let message = format!("{name}! formats its message; runner.log keeps only literals");
            self.found
                .push(checks::violation(self.path, line, message, Fix::Literal));
        }
        syn::visit::visit_macro(self, mac);
    }
}

/// True when a `tracing` event's arguments `tokens` format the message: the message, the first
/// argument that is one string literal alone, holds a `{` that is not `{{`, or an argument
/// follows it. A field (`name = value`, `%value`, `?value`) or `target: "x"` is never alone.
pub(super) fn formatted(tokens: &TokenStream) -> bool {
    let mut parts: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for tree in tokens.clone() {
        match &tree {
            TokenTree::Punct(punct) if punct.as_char() == ',' => parts.push(Vec::new()),
            _ => {
                if let Some(part) = parts.last_mut() {
                    part.push(tree);
                }
            }
        }
    }
    let message = parts
        .iter()
        .enumerate()
        .find_map(|(at, part)| match part.as_slice() {
            [TokenTree::Literal(literal)] => syn::parse_str::<syn::LitStr>(&literal.to_string())
                .ok()
                .map(|lit| (at, lit.value())),
            _ => None,
        });
    let Some((at, text)) = message else {
        return false;
    };
    let after = parts.get(at + 1..).unwrap_or_default();
    text.replace("{{", "").contains('{') || after.iter().any(|part| !part.is_empty())
}

#[cfg(test)]
mod tests;
