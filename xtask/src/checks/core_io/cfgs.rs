//! The cfgs clippy never leaves off in a held crate: clippy lints it with `cfg(clippy)` set and
//! with `debug_assertions` on, so code behind `not(clippy)` or `not(debug_assertions)` is never
//! linted. Each `.rs` file of the crate is lexed, macro bodies included, and every identifier
//! `clippy` or `debug_assertions` fails, but `clippy` directly before `::`, a lint path.

use super::report;
use crate::checks;
use crate::violation::Violation;
use crate::workspace::{self, Member};
use proc_macro2::{Spacing, TokenStream, TokenTree};
use std::path::Path;
use std::str::FromStr;

/// The cfg names that clippy always sets.
const ALWAYS_SET: [&str; 2] = ["clippy", "debug_assertions"];

/// The violations for the held crate `member`'s `.rs` files among `files` (git's list): each
/// file that does not lex, and each use of a name in `ALWAYS_SET` outside a lint path.
pub(super) fn always_set_cfgs(root: &Path, member: &Member, files: &[String]) -> Vec<Violation> {
    let mut found = Vec::new();
    let sources = files
        .iter()
        .filter(|file| file.ends_with(".rs") && workspace::is_under(file, &member.dir));
    for path in sources {
        let text = match checks::read_repo_file(root, path) {
            Ok(Some(text)) => text,
            // git listed it; a file gone since then is no code to lint.
            Ok(None) => continue,
            Err(unreadable) => {
                found.push(unreadable);
                continue;
            }
        };
        match TokenStream::from_str(&text) {
            Ok(tokens) => {
                let mut hits = Vec::new();
                walk(tokens, &mut hits);
                for (name, line) in hits {
                    found.push(report::always_set_cfg(path, line, &name, &member.name));
                }
            }
            Err(error) => {
                let line = line_of(error.span().start().line);
                found.push(report::source_not_lexed(path, line, &member.name));
            }
        }
    }
    found
}

/// Pushes onto `hits` the name and line of each identifier in `tokens`, groups included, that
/// is in `ALWAYS_SET`, but `clippy` directly followed by `::`.
fn walk(tokens: TokenStream, hits: &mut Vec<(String, u32)>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(group) => walk(group.stream(), hits),
            TokenTree::Ident(ident) => {
                let text = ident.to_string();
                let name = text.strip_prefix("r#").unwrap_or(&text);
                if !ALWAYS_SET.contains(&name) {
                    continue;
                }
                let lint_path = name == "clippy" && is_path_separator(&trees[index + 1..]);
                if !lint_path {
                    hits.push((name.to_owned(), line_of(ident.span().start().line)));
                }
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// True when `rest` starts with `::`, two `:` with nothing between them.
fn is_path_separator(rest: &[TokenTree]) -> bool {
    match rest {
        [TokenTree::Punct(first), TokenTree::Punct(second), ..] => {
            first.as_char() == ':' && first.spacing() == Spacing::Joint && second.as_char() == ':'
        }
        _ => false,
    }
}

/// A 1-based line from a span's line.
fn line_of(line: usize) -> u32 {
    u32::try_from(line).unwrap_or(u32::MAX).max(1)
}
