//! One file's tokens, as the unix-paths check reads them: every string literal (raw strings
//! included) that starts with a prefix the check counts, with what exempts it, and every
//! out-of-line `mod name;`, so `modules.rs` can carry a cfg on that line into the module's file.
//!
//! The file is lexed with core-io's lexer (`cfgs::lex`), so comments are gone, and a doc comment
//! is a `#[doc = ".."]` attribute, which the walk skips. A `cfg` on an item, a statement, a field
//! or a match arm covers the tokens up to its end (`item_end`); a `#![cfg]` covers the rest of the
//! file or module block it opens.

use crate::checks::core_io::cfgs::{ident_name, is_punct, lex, line_of};
use proc_macro2::{Delimiter, LexError, TokenStream, TokenTree};

/// A string literal that starts with one of the check's prefixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Literal {
    pub(super) line: u32,
    pub(super) value: String,
    /// True when a cfg keeps it out of every Windows build (`unix_only`).
    pub(super) unix: bool,
    /// True when it is inside the parentheses of `testing::abs(..)` or a bare `abs(..)`.
    pub(super) abs: bool,
    /// True when a cfg compiles it only in the test build, such as `#[cfg(test)] mod tests`.
    pub(super) test: bool,
}

/// An out-of-line `mod name;`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModDecl {
    pub(super) name: String,
    /// The value of its `#[path = ".."]`, if it has one.
    pub(super) path: Option<String>,
    /// The inline `mod a { .. }` blocks around it, outermost first.
    pub(super) inline: Vec<String>,
    /// True when a cfg on it, or around it, keeps it out of every Windows build.
    pub(super) unix: bool,
}

/// What one file holds.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct FileScan {
    pub(super) literals: Vec<Literal>,
    pub(super) mods: Vec<ModDecl>,
}

/// The scan of `text`, or the lexer's error.
pub(super) fn scan(text: &str, prefixes: &[&str]) -> Result<FileScan, LexError> {
    let tokens = lex(text)?;
    let mut found = FileScan::default();
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut walker = Walker {
        prefixes,
        found: &mut found,
    };
    walker.sequence(&trees, &Ctx::default());
    Ok(found)
}

/// What covers the tokens being walked.
#[derive(Debug, Default, Clone)]
struct Ctx {
    unix: bool,
    test: bool,
    abs: bool,
    inline: Vec<String>,
}

/// The attributes read ahead of an item.
#[derive(Debug, Default)]
struct Pending {
    unix: bool,
    test: bool,
    path: Option<String>,
}

struct Walker<'a> {
    prefixes: &'a [&'a str],
    found: &'a mut FileScan,
}

impl Walker<'_> {
    /// Walks `trees`, a file or the inside of a group: attributes, then the item they cover.
    fn sequence(&mut self, trees: &[TokenTree], outer: &Ctx) {
        let mut ctx = outer.clone();
        let mut pending = Pending::default();
        let mut index = 0;
        while index < trees.len() {
            if let Some((inner, brackets, next)) = attribute_at(trees, index) {
                let parts: Vec<TokenTree> = brackets.into_iter().collect();
                let head = match parts.first() {
                    Some(TokenTree::Ident(ident)) => ident_name(ident),
                    _ => String::new(),
                };
                match head.as_str() {
                    "doc" => {}
                    "cfg" => {
                        let predicate = parse_group(parts.get(1));
                        let target = if inner {
                            (&mut ctx.unix, &mut ctx.test)
                        } else {
                            (&mut pending.unix, &mut pending.test)
                        };
                        *target.0 |= predicate.never_on_windows();
                        *target.1 |= predicate.requires_test();
                    }
                    "path" => pending.path = string_value(parts.get(2)),
                    _ => self.sequence(&parts, &ctx),
                }
                index = next;
                continue;
            }
            let end = item_end(trees, index);
            let item_ctx = Ctx {
                unix: ctx.unix || pending.unix,
                test: ctx.test || pending.test,
                ..ctx.clone()
            };
            self.item(&trees[index..end], &item_ctx, pending.path.take());
            pending = Pending::default();
            index = end;
        }
    }

    /// Walks one item, statement, field or arm, `trees`, in `ctx`; `path` is its `#[path]`.
    fn item(&mut self, trees: &[TokenTree], ctx: &Ctx, path: Option<String>) {
        let rest = skip_visibility(trees);
        if let [TokenTree::Ident(keyword), TokenTree::Ident(name), after, ..] = rest
            && ident_name(keyword) == "mod"
        {
            let name = ident_name(name);
            if is_punct(after, ';') {
                self.found.mods.push(ModDecl {
                    name,
                    path,
                    inline: ctx.inline.clone(),
                    unix: ctx.unix,
                });
                return;
            }
            if let TokenTree::Group(body) = after
                && body.delimiter() == Delimiter::Brace
            {
                let mut inline = ctx.clone();
                inline.inline.push(name);
                let trees: Vec<TokenTree> = body.stream().into_iter().collect();
                self.sequence(&trees, &inline);
                return;
            }
        }
        for (index, tree) in trees.iter().enumerate() {
            match tree {
                TokenTree::Literal(_) => self.literal(tree, ctx),
                TokenTree::Group(group) => {
                    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                    if group.delimiter() == Delimiter::Parenthesis && calls_abs(&trees[..index]) {
                        let abs = Ctx {
                            abs: true,
                            ..ctx.clone()
                        };
                        self.sequence(&inner, &abs);
                    } else {
                        self.sequence(&inner, ctx);
                    }
                }
                TokenTree::Ident(_) | TokenTree::Punct(_) => {}
            }
        }
    }

    /// Keeps `tree` when it is a string literal that starts with one of the prefixes.
    fn literal(&mut self, tree: &TokenTree, ctx: &Ctx) {
        let Some(value) = string_value(Some(tree)) else {
            return;
        };
        if self.prefixes.iter().any(|prefix| value.starts_with(prefix)) {
            self.found.literals.push(Literal {
                line: line_of(tree.span().start().line),
                value,
                unix: ctx.unix,
                abs: ctx.abs,
                test: ctx.test,
            });
        }
    }
}

/// When an attribute starts at `index`: whether it is inner, its bracket contents, and the
/// index after it.
fn attribute_at(trees: &[TokenTree], index: usize) -> Option<(bool, TokenStream, usize)> {
    if !is_punct(trees.get(index)?, '#') {
        return None;
    }
    let inner = trees.get(index + 1).is_some_and(|tree| is_punct(tree, '!'));
    let at = index + 1 + usize::from(inner);
    match trees.get(at)? {
        TokenTree::Group(group) if group.delimiter() == Delimiter::Bracket => {
            Some((inner, group.stream(), at + 1))
        }
        _ => None,
    }
}

/// The value of `tree` when it is a string literal, raw or not.
fn string_value(tree: Option<&TokenTree>) -> Option<String> {
    let TokenTree::Literal(literal) = tree? else {
        return None;
    };
    let tokens = TokenStream::from(TokenTree::Literal(literal.clone()));
    syn::parse2::<syn::LitStr>(tokens)
        .ok()
        .map(|lit| lit.value())
}

/// `trees` after a leading `pub` or `pub(..)`.
fn skip_visibility(trees: &[TokenTree]) -> &[TokenTree] {
    match trees {
        [TokenTree::Ident(word), TokenTree::Group(scope), rest @ ..]
            if ident_name(word) == "pub" && scope.delimiter() == Delimiter::Parenthesis =>
        {
            rest
        }
        [TokenTree::Ident(word), rest @ ..] if ident_name(word) == "pub" => rest,
        _ => trees,
    }
}

/// True when `before`, the trees ahead of a parenthesis group, end in a bare `abs` or in
/// `testing::abs`, so the group holds the helper's argument. A method `.abs()` is not it.
fn calls_abs(before: &[TokenTree]) -> bool {
    match before {
        [
            ..,
            TokenTree::Ident(owner),
            colon,
            colon2,
            TokenTree::Ident(name),
        ] if is_punct(colon, ':') && is_punct(colon2, ':') => {
            ident_name(name) == "abs" && ident_name(owner) == "testing"
        }
        [.., TokenTree::Punct(punct), TokenTree::Ident(_)]
            if matches!(punct.as_char(), '.' | ':') =>
        {
            false
        }
        [.., TokenTree::Ident(name)] => ident_name(name) == "abs",
        _ => false,
    }
}

/// The keywords after which an item ends at its first brace group.
const BLOCK_ITEMS: [&str; 8] = [
    "fn",
    "mod",
    "impl",
    "trait",
    "struct",
    "enum",
    "union",
    "macro_rules",
];

/// The modifiers that may come before an item's keyword.
const MODIFIERS: [&str; 6] = ["pub", "unsafe", "async", "extern", "const", "default"];

/// The index just past the item that starts at `start`: a block item (`BLOCK_ITEMS`) ends at its
/// first brace group or `;`. Anything else ends at `;` or `,`, or at a brace group with no `=`
/// before it (a block statement), or right after `=> { .. }` (a match arm).
fn item_end(trees: &[TokenTree], start: usize) -> usize {
    let block = trees[start..]
        .iter()
        .filter_map(|tree| match tree {
            TokenTree::Ident(ident) => Some(ident_name(ident)),
            TokenTree::Group(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => None,
        })
        .find(|word| !MODIFIERS.contains(&word.as_str()))
        .is_some_and(|word| BLOCK_ITEMS.contains(&word.as_str()));
    let mut seen_eq = false;
    let mut arrow = false;
    for (offset, tree) in trees[start..].iter().enumerate() {
        let end = start + offset + 1;
        match tree {
            TokenTree::Punct(punct) if punct.as_char() == ';' => return end,
            TokenTree::Punct(punct) if punct.as_char() == ',' && !block => return end,
            TokenTree::Punct(punct) if punct.as_char() == '=' => {
                seen_eq = true;
                arrow = trees.get(end).is_some_and(|next| is_punct(next, '>'));
            }
            TokenTree::Group(group)
                if group.delimiter() == Delimiter::Brace && (block || !seen_eq || arrow) =>
            {
                return end;
            }
            _ => {}
        }
    }
    trees.len()
}

/// A cfg predicate, as far as the check reads it.
#[derive(Debug)]
enum Predicate {
    Name(String),
    Equals(String, String),
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    Not(Box<Predicate>),
    Other,
}

/// The predicate inside `group`, the parentheses after `cfg`.
fn parse_group(group: Option<&TokenTree>) -> Predicate {
    match group {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
            let trees: Vec<TokenTree> = group.stream().into_iter().collect();
            let trees = match trees.as_slice() {
                [most @ .., last] if is_punct(last, ',') => most,
                all => all,
            };
            parse(trees)
        }
        _ => Predicate::Other,
    }
}

/// The predicate `trees` spell.
fn parse(trees: &[TokenTree]) -> Predicate {
    match trees {
        [TokenTree::Ident(name)] => Predicate::Name(ident_name(name)),
        [TokenTree::Ident(name), eq, value] if is_punct(eq, '=') => {
            match string_value(Some(value)) {
                Some(value) => Predicate::Equals(ident_name(name), value),
                None => Predicate::Other,
            }
        }
        [TokenTree::Ident(name), TokenTree::Group(group)]
            if group.delimiter() == Delimiter::Parenthesis =>
        {
            let inner: Vec<TokenTree> = group.stream().into_iter().collect();
            let parts: Vec<Predicate> = inner
                .split(|tree| is_punct(tree, ','))
                .filter(|part| !part.is_empty())
                .map(parse)
                .collect();
            match ident_name(name).as_str() {
                "all" => Predicate::All(parts),
                "any" => Predicate::Any(parts),
                "not" => match <[Predicate; 1]>::try_from(parts) {
                    Ok([only]) => Predicate::Not(Box::new(only)),
                    Err(_) => Predicate::Other,
                },
                _ => Predicate::Other,
            }
        }
        _ => Predicate::Other,
    }
}

impl Predicate {
    /// True when the predicate is false in every Windows build: `unix`, `not(windows)`, a
    /// `target_os` or `target_family` other than `windows`, and `all` or `any` of those.
    fn never_on_windows(&self) -> bool {
        match self {
            Predicate::Name(name) => name == "unix",
            Predicate::Equals(key, value) => {
                matches!(key.as_str(), "target_os" | "target_family") && value != "windows"
            }
            Predicate::All(parts) => parts.iter().any(Predicate::never_on_windows),
            Predicate::Any(parts) => {
                !parts.is_empty() && parts.iter().all(Predicate::never_on_windows)
            }
            Predicate::Not(inner) => inner.always_on_windows(),
            Predicate::Other => false,
        }
    }

    /// True when the predicate holds in every Windows build.
    fn always_on_windows(&self) -> bool {
        match self {
            Predicate::Name(name) => name == "windows",
            Predicate::Equals(key, value) => {
                matches!(key.as_str(), "target_os" | "target_family") && value == "windows"
            }
            Predicate::All(parts) => {
                !parts.is_empty() && parts.iter().all(Predicate::always_on_windows)
            }
            Predicate::Any(parts) => parts.iter().any(Predicate::always_on_windows),
            Predicate::Not(inner) => inner.never_on_windows(),
            Predicate::Other => false,
        }
    }

    /// True when the predicate holds only in the test build.
    fn requires_test(&self) -> bool {
        match self {
            Predicate::Name(name) => name == "test",
            Predicate::All(parts) => parts.iter().any(Predicate::requires_test),
            Predicate::Any(parts) => {
                !parts.is_empty() && parts.iter().all(Predicate::requires_test)
            }
            Predicate::Equals(..) | Predicate::Not(_) | Predicate::Other => false,
        }
    }
}

#[cfg(test)]
mod tests;
