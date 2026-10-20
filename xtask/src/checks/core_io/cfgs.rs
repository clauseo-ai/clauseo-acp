//! The cfg rule of a held crate. clippy lints the crate once per target kind with every feature
//! on, so code is linted only when its cfg holds in that build. `test` holds in the test build,
//! `not(test)` in the others, and a positive feature holds under `--all-features`. Any other
//! name, or a feature under a `not`, can leave code out of every clippy run.
//!
//! Each `.rs` file of the crate is lexed, and every token tree is walked, macro bodies and macro
//! call arguments included. The predicate of each `cfg` and `cfg_attr` at the start of any
//! bracket group or inside a `cfg_attr` is built only from `all`, `any`, `not`, `test` and
//! `feature = "<literal>"`, with no `feature` under a `not`. Any bracket group counts, since a
//! macro can join a `#` to brackets passed separately. The name `cfg_select` fails wherever it
//! stands, a `use` included, since `cfg_select!` builds only the arm whose cfg holds and an alias
//! would hide the call. `cfg!` passes, since both branches of an `if cfg!(..)` are compiled and
//! linted. An attribute or a macro
//! call whose name is a metavariable fails, and so does a `#` before a metavariable, since a macro
//! could build a `cfg` or an `include!` from a passed name. A `path` attribute, `include!` and
//! any symlink under the crate's folder fail, so every module file sits at its default path
//! under the crate, ends in `.rs` and is lexed here.

use super::report;
use crate::checks;
use crate::violation::Violation;
use crate::workspace::{self, Member};
use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};
use std::path::Path;
use std::str::FromStr;

/// What a held crate's file may not hold, found at a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Finding {
    /// A cfg predicate with a form other than `all`, `any`, `not`, `test` and a positive feature,
    /// described as the fix names it.
    Predicate(String),
    /// An attribute whose name is a metavariable, or a `#` before a metavariable.
    MetavariableAttribute,
    /// A `path` attribute.
    PathAttribute,
    /// A call of `include!`.
    Include,
    /// The name `cfg_select`, whose macro builds only the arm whose cfg holds.
    CfgSelect,
    /// A macro call whose name is a metavariable, such as `$x!`.
    MetavariableMacro,
}

/// The violations for the held crate `member`'s entries among `entries` (git's list, a symlink
/// as one entry): each symlink, each `.rs` file that does not lex, and each `Finding` in the
/// others.
pub(super) fn cfg_rule(root: &Path, member: &Member, entries: &[String]) -> Vec<Violation> {
    let mut found = Vec::new();
    let mut sources = Vec::new();
    for entry in entries
        .iter()
        .filter(|entry| workspace::is_under(entry, &member.dir))
    {
        match root.join(entry).symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                found.push(report::symlink(entry, &member.name));
            }
            Ok(meta) if meta.is_file() && is_rust_file(entry) => sources.push(entry),
            // A tracked entry gone from disk is no code to lint; any other error is unreadable.
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => found.push(checks::unreadable(entry, &error)),
        }
    }
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
                for (finding, line) in hits {
                    found.push(report::cfg_rule(path, line, &finding, &member.name));
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

/// True when `file` ends in `.rs` in any case, which a case-insensitive file system opens for a
/// module.
fn is_rust_file(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}

/// Pushes onto `hits` each `Finding` in `tokens` and its line, groups included.
fn walk(tokens: TokenStream, hits: &mut Vec<(Finding, u32)>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        let rest = &trees[index + 1..];
        match tree {
            TokenTree::Group(group) => {
                if group.delimiter() == Delimiter::Bracket {
                    attribute_head(&group.stream().into_iter().collect::<Vec<_>>(), hits);
                }
                walk(group.stream(), hits);
            }
            TokenTree::Ident(ident) => {
                let line = line_of(ident.span().start().line);
                match ident_name(ident).as_str() {
                    "cfg_select" => hits.push((Finding::CfgSelect, line)),
                    // A `$` after the name may stand for the `!`, so it counts as a call.
                    "include" if starts_with_punct(rest, '!') || starts_with_punct(rest, '$') => {
                        hits.push((Finding::Include, line));
                    }
                    _ => {}
                }
            }
            TokenTree::Punct(dollar) if dollar.as_char() == '$' && names_a_macro(rest) => {
                hits.push((
                    Finding::MetavariableMacro,
                    line_of(dollar.span().start().line),
                ));
            }
            TokenTree::Punct(punct) if punct.as_char() == '#' => {
                let rest = if starts_with_punct(rest, '!') {
                    &rest[1..]
                } else {
                    rest
                };
                match rest.first() {
                    Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket => {
                        if let Some(first) = group.stream().into_iter().next() {
                            metavariable_name(&first, hits);
                        }
                    }
                    Some(other) => metavariable_name(other, hits),
                    None => {}
                }
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// True when `rest`, which follows a `$`, makes the metavariable a macro's name: `$x!`, or a
/// repetition such as `$($x)*!`.
fn names_a_macro(rest: &[TokenTree]) -> bool {
    match rest {
        [TokenTree::Ident(_), after @ ..] => starts_with_punct(after, '!'),
        [TokenTree::Group(_), after @ ..] => {
            let after = match after {
                [op, more @ ..] if is_punct(op, '*') || is_punct(op, '+') || is_punct(op, '?') => {
                    more
                }
                _ => after,
            };
            starts_with_punct(after, '!')
        }
        _ => false,
    }
}

/// Checks what follows `cfg` in an attribute, a parenthesized predicate.
fn cfg_group(rest: &[TokenTree], hits: &mut Vec<(Finding, u32)>) {
    match rest.first() {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
            let trees: Vec<TokenTree> = group.stream().into_iter().collect();
            if let Err(hit) = whole_predicate(&trees, group) {
                hits.push(hit);
            }
        }
        Some(other) => metavariable_in_place(other, hits),
        None => {}
    }
}

/// Pushes a metavariable finding when `tree`, which stands where a cfg's group belongs, is `$`.
fn metavariable_in_place(tree: &TokenTree, hits: &mut Vec<(Finding, u32)>) {
    if is_punct(tree, '$') {
        hits.push((metavariable(), line_of(tree.span().start().line)));
    }
}

/// The finding for a metavariable in place of a predicate.
fn metavariable() -> Finding {
    Finding::Predicate("a metavariable".to_owned())
}

/// Checks what follows `cfg_attr` in an attribute, `rest`: in its parenthesized group the first
/// comma-separated part is a predicate, and each attribute after it is checked as one.
fn cfg_attr(rest: &[TokenTree], hits: &mut Vec<(Finding, u32)>) {
    let group = match rest.first() {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => group,
        Some(other) => return metavariable_in_place(other, hits),
        None => return,
    };
    let trees: Vec<TokenTree> = group.stream().into_iter().collect();
    let mut parts = trees.split(|tree| is_punct(tree, ','));
    let predicate = parts.next().unwrap_or_default();
    if let Err(hit) = whole_predicate(predicate, group) {
        hits.push(hit);
    }
    for part in parts {
        attribute(part, hits);
    }
}

/// Checks the tokens of one attribute after the predicate of `cfg_attr`: its name may not be a
/// metavariable, and its head is checked as `attribute_head` checks it.
fn attribute(tokens: &[TokenTree], hits: &mut Vec<(Finding, u32)>) {
    if let Some(first) = tokens.first() {
        metavariable_name(first, hits);
    }
    attribute_head(tokens, hits);
}

/// Pushes a metavariable attribute finding when `tree`, which stands where an attribute's name
/// belongs, is `$`.
fn metavariable_name(tree: &TokenTree, hits: &mut Vec<(Finding, u32)>) {
    if is_punct(tree, '$') {
        hits.push((
            Finding::MetavariableAttribute,
            line_of(tree.span().start().line),
        ));
    }
}

/// Checks `tokens` as the inside of an attribute, whether or not a `#` stands before them, since a
/// macro can put the `#` and the brackets together from separate tokens. A `path = ..` or
/// `path $` fails, and a `cfg` or `cfg_attr` predicate must be allowed.
fn attribute_head(tokens: &[TokenTree], hits: &mut Vec<(Finding, u32)>) {
    let Some(TokenTree::Ident(ident)) = tokens.first() else {
        return;
    };
    match ident_name(ident).as_str() {
        // A `$` after `path` may stand for the `=`.
        "path" if starts_with_punct(&tokens[1..], '=') || starts_with_punct(&tokens[1..], '$') => {
            hits.push((Finding::PathAttribute, line_of(ident.span().start().line)));
        }
        "cfg" => cfg_group(&tokens[1..], hits),
        "cfg_attr" => cfg_attr(&tokens[1..], hits),
        _ => {}
    }
}

/// `Ok` when `trees`, the contents of `group`, are one allowed predicate, with an optional
/// trailing comma; else the first `Finding` and its line.
fn whole_predicate(trees: &[TokenTree], group: &Group) -> Result<(), (Finding, u32)> {
    let trees = match trees {
        [most @ .., last] if is_punct(last, ',') => most,
        _ => trees,
    };
    if trees.is_empty() {
        return Err(not_a_predicate(line_of(group.span().start().line)));
    }
    predicate(trees, false)
}

/// `Ok` when `trees` are exactly one allowed predicate; `negated` is true under a `not`.
fn predicate(trees: &[TokenTree], negated: bool) -> Result<(), (Finding, u32)> {
    let first = &trees[0];
    let line = line_of(first.span().start().line);
    let TokenTree::Ident(ident) = first else {
        if is_punct(first, '$') {
            return Err((metavariable(), line));
        }
        return Err(not_a_predicate(line));
    };
    let name = ident_name(ident);
    match (name.as_str(), &trees[1..]) {
        ("test", []) => Ok(()),
        ("feature", [TokenTree::Punct(eq), TokenTree::Literal(literal)])
            if eq.as_char() == '=' && literal.to_string().starts_with(['"', 'r']) =>
        {
            if negated {
                Err((Finding::Predicate("a feature under `not`".to_owned()), line))
            } else {
                Ok(())
            }
        }
        ("all" | "any" | "not", [TokenTree::Group(group)])
            if group.delimiter() == Delimiter::Parenthesis =>
        {
            let inner: Vec<TokenTree> = group.stream().into_iter().collect();
            let mut parts: Vec<&[TokenTree]> = inner.split(|tree| is_punct(tree, ',')).collect();
            if parts.last().is_some_and(|part| part.is_empty()) {
                parts.pop();
            }
            if name == "not" && parts.len() != 1 {
                return Err(not_a_predicate(line));
            }
            for part in parts {
                if part.is_empty() {
                    return Err(not_a_predicate(line));
                }
                predicate(part, negated || name == "not")?;
            }
            Ok(())
        }
        ("feature" | "all" | "any" | "not" | "test", _) => Err(not_a_predicate(line)),
        _ => Err((Finding::Predicate(format!("the name `{name}`")), line)),
    }
}

/// The finding for tokens at `line` that are not a cfg predicate.
fn not_a_predicate(line: u32) -> (Finding, u32) {
    (
        Finding::Predicate("a form that is not a predicate".to_owned()),
        line,
    )
}

/// The name of `ident`, without the `r#` of a raw identifier.
fn ident_name(ident: &proc_macro2::Ident) -> String {
    let text = ident.to_string();
    text.strip_prefix("r#")
        .map_or_else(|| text.clone(), str::to_owned)
}

/// True when `tree` is the punctuation `ch`.
fn is_punct(tree: &TokenTree, ch: char) -> bool {
    matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ch)
}

/// True when `rest` starts with the punctuation `ch`.
fn starts_with_punct(rest: &[TokenTree], ch: char) -> bool {
    rest.first().is_some_and(|tree| is_punct(tree, ch))
}

/// A 1-based line from a span's line.
fn line_of(line: usize) -> u32 {
    u32::try_from(line).unwrap_or(u32::MAX).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The findings of `source`, each as its kind and line.
    fn findings(source: &str) -> Vec<(Finding, u32)> {
        let mut hits = Vec::new();
        walk(
            TokenStream::from_str(source).expect("the source lexes"),
            &mut hits,
        );
        hits
    }

    /// Forms the planted cases do not show: other delimiters, a metavariable in place of the
    /// group, a nested `not`, and predicates that are malformed.
    #[test]
    fn forms_beyond_the_planted_cases() {
        let name = |name: &str| Finding::Predicate(format!("the name `{name}`"));
        let malformed = || Finding::Predicate("a form that is not a predicate".to_owned());
        let negated = || Finding::Predicate("a feature under `not`".to_owned());
        let cases = [
            ("cfg!{unix}; if cfg!(not(clippy)) {}", vec![]),
            (
                "macro_rules! cfg_select { () => {} }",
                vec![(Finding::CfgSelect, 1)],
            ),
            ("#[cfg $p]", vec![(metavariable(), 1)]),
            ("#[cfg_attr $p]", vec![(metavariable(), 1)]),
            ("#[cfg_attr($p, derive(Debug))]", vec![(metavariable(), 1)]),
            ("#![$a(x)]", vec![(Finding::MetavariableAttribute, 1)]),
            (
                "#[cfg_attr(test, $a)]",
                vec![(Finding::MetavariableAttribute, 1)],
            ),
            ("#[cfg(not(not(feature = \"a\")))]", vec![(negated(), 1)]),
            (
                "#[cfg(not(all(test, feature = \"a\")))]",
                vec![(negated(), 1)],
            ),
            ("#[cfg(feature)]", vec![(malformed(), 1)]),
            ("#[cfg(test, unix)]", vec![(malformed(), 1)]),
            ("#[cfg(not(test, test))]", vec![(malformed(), 1)]),
            ("#[cfg()]", vec![(malformed(), 1)]),
            (
                "#[cfg_attr(test, cfg_attr(unix, derive(Debug)))]",
                vec![(name("unix"), 1)],
            ),
            ("r#include!(\"x.rs\");", vec![(Finding::Include, 1)]),
            (
                "#[cfg(test,)] #[cfg(any())] #[cfg(all(test,))] fn f() {}",
                vec![],
            ),
            ("let cfg = 1; include_bytes!(\"x\"); path::x();", vec![]),
            ("self.cfg(); fn cfg(x: u8) {} cfg_attr(unix);", vec![]),
            (
                "let a = [path, cfg]; vec![$x]; m!([path = \"x\"]);",
                vec![(Finding::PathAttribute, 1)],
            ),
            ("#!$a", vec![(Finding::MetavariableAttribute, 1)]),
            ("cfg $b (unix)", vec![]),
            (
                "$($m)*!(x); $crate::m!(); $i::x!();",
                vec![(Finding::MetavariableMacro, 1)],
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(findings(source), expected, "{source}");
        }
    }
}
