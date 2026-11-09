//! What `log-literals` judges in one file: each `warn!`, `error!` and `event!` call, each `use`
//! that renames one of them, and each `macro_rules!` that calls one of them.

use super::Fix;
use crate::checks;
use crate::violation::Violation;
use proc_macro2::{Spacing, Span, TokenStream, TokenTree};
use syn::visit::Visit;

/// The macros the check reads: the two levels `runner.log` keeps, and `event!`, which takes the
/// level as its first argument.
const MACROS: [&str; 3] = ["warn", "error", "event"];
/// The levels `runner.log` drops: an `event!` at one of them passes.
const QUIET: [&str; 3] = ["TRACE", "DEBUG", "INFO"];
/// The keys an `event!` may take before its level (`target: "x"`, `parent: span`, `name: "x"`).
const KEYS: [&str; 3] = ["target", "parent", "name"];

/// The calls of one file, and the violations among them.
pub(super) struct Calls<'a> {
    path: &'a str,
    pub(super) found: Vec<Violation>,
}

impl<'a> Calls<'a> {
    pub(super) fn new(path: &'a str) -> Calls<'a> {
        Calls {
            path,
            found: Vec::new(),
        }
    }

    fn push(&mut self, at: Span, message: String, fix: Fix) {
        let line = u32::try_from(at.start().line).unwrap_or(1).max(1);
        self.found
            .push(checks::violation(self.path, line, message, fix));
    }
}

impl<'ast> Visit<'ast> for Calls<'_> {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let at = mac
            .path
            .segments
            .first()
            .map_or_else(|| mac.bang_token.span, |segment| segment.ident.span());
        let name = mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        if name == "macro_rules" {
            if let Some(called) = calls_one(&mac.tokens) {
                let message =
                    format!("a macro_rules! body calls {called}!, which log-literals can't follow");
                self.push(at, message, Fix::Wrapper);
            }
        } else if MACROS.contains(&name.as_str())
            && let Some(why) = judge(&name, &mac.tokens)
        {
            let message = format!("{name}! {why}; runner.log keeps only literals");
            self.push(at, message, Fix::Literal);
        }
        syn::visit::visit_macro(self, mac);
    }

    fn visit_use_rename(&mut self, rename: &'ast syn::UseRename) {
        let name = rename.ident.to_string();
        if MACROS.contains(&name.as_str()) {
            let message = format!(
                "`{name}` is renamed `{}`, which log-literals can't follow",
                rename.rename
            );
            self.push(rename.ident.span(), message, Fix::Alias);
        }
        syn::visit::visit_use_rename(self, rename);
    }
}

/// The macro of [`MACROS`] that `tokens` (a `macro_rules!` body) calls, at any depth.
fn calls_one(tokens: &TokenStream) -> Option<String> {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Ident(ident) if MACROS.contains(&ident.to_string().as_str()) => {
                if let Some(TokenTree::Punct(bang)) = trees.get(at + 1)
                    && bang.as_char() == '!'
                {
                    return Some(ident.to_string());
                }
            }
            TokenTree::Group(group) => {
                if let Some(called) = calls_one(&group.stream()) {
                    return Some(called);
                }
            }
            _ => {}
        }
    }
    None
}

/// What is wrong with a call of `name` whose arguments are `tokens`: its message is a field, or
/// is formatted. `None` for a literal message, no message, or an `event!` at a [`QUIET`] level.
pub(super) fn judge(name: &str, tokens: &TokenStream) -> Option<&'static str> {
    let parts = split(tokens);
    let mut rest = parts.as_slice();
    if name == "event" {
        while let Some((first, after)) = rest.split_first()
            && is_keyed(first)
        {
            rest = after;
        }
        let (level, after) = rest.split_first()?;
        if let Some(TokenTree::Ident(last)) = level.last()
            && QUIET.contains(&last.to_string().as_str())
        {
            return None;
        }
        rest = after;
    }
    if rest.iter().any(|part| is_message_field(part)) {
        return Some("sets its message as a field");
    }
    formatted(rest).then_some("formats its message")
}

/// `tokens` split at its top-level commas.
fn split(tokens: &TokenStream) -> Vec<Vec<TokenTree>> {
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
    parts
}

/// True for `target: ...`, `parent: ...` or `name: ...`: one `:`, not a path's `::`.
fn is_keyed(part: &[TokenTree]) -> bool {
    matches!(
        part,
        [TokenTree::Ident(key), TokenTree::Punct(colon), ..]
            if KEYS.contains(&key.to_string().as_str())
                && colon.as_char() == ':'
                && colon.spacing() == Spacing::Alone
    )
}

/// True for a field named `message`: `message = ...`, `"message" = ...`, `%message`, `?message`
/// or `message` alone, each of which `tracing` takes as the event's message.
fn is_message_field(part: &[TokenTree]) -> bool {
    let rest = match part {
        [TokenTree::Punct(sigil), rest @ ..] if matches!(sigil.as_char(), '%' | '?') => rest,
        _ => part,
    };
    let set = matches!(rest.get(1), Some(TokenTree::Punct(eq)) if eq.as_char() == '=');
    match rest.first() {
        Some(TokenTree::Ident(ident)) => ident == "message" && (set || rest.len() == 1),
        Some(TokenTree::Literal(literal)) => set && literal.to_string() == "\"message\"",
        _ => false,
    }
}

/// True when the event's arguments `parts` format the message: the message, the first part that
/// is one string literal alone, holds a `{` that is not `{{`, or a part follows it. A field
/// (`name = value`, `%value`, `?value`) or `target: "x"` is never alone.
fn formatted(parts: &[Vec<TokenTree>]) -> bool {
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
