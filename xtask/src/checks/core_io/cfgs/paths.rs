//! The `path` attributes that core-io can follow. rustc resolves `#[path = ".."]` on an
//! out-of-line `mod name;` outside any inline module block against the folder of the file that
//! holds it. `allowed` tells whether brackets at the top level of a file are such an attribute
//! with a string literal, and `resolve` follows its value from that folder one component at a
//! time.

use super::{ident_name, is_punct};
use crate::workspace;
use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};
use std::collections::BTreeSet;

/// The file the walk is in at its top level, and what a `path` attribute there may name.
pub(super) struct TopLevel<'a> {
    /// The file, repo-relative.
    pub(super) file: &'a str,
    /// The crate's folder, repo-relative.
    pub(super) dir: &'a str,
    /// Every file core-io lexes in the crate, repo-relative.
    pub(super) lexed: &'a BTreeSet<&'a str>,
}

/// True when `brackets`, which follow a `#` at the top level of `top.file`, are
/// `[path = "<literal>"]`, `after` declares an out-of-line module, and the literal resolves to a
/// file in `top.lexed` without leaving `top.dir`.
pub(super) fn allowed(brackets: &Group, after: &[TokenTree], top: &TopLevel<'_>) -> bool {
    let parts: Vec<TokenTree> = brackets.stream().into_iter().collect();
    let [TokenTree::Ident(name), eq, TokenTree::Literal(literal)] = parts.as_slice() else {
        return false;
    };
    if ident_name(name) != "path" || !is_punct(eq, '=') || !declares_module(after) {
        return false;
    }
    let literal = TokenStream::from(TokenTree::Literal(literal.clone()));
    let Ok(value) = syn::parse2::<syn::LitStr>(literal) else {
        return false;
    };
    resolve(&value.value(), top.file, top.dir).is_some_and(|target| top.lexed.contains(&*target))
}

/// True when `after`, the trees after an attribute, are more outer attributes, an optional
/// visibility, then `mod name;`.
fn declares_module(after: &[TokenTree]) -> bool {
    let mut rest = after;
    while let [hash, TokenTree::Group(brackets), more @ ..] = rest
        && is_punct(hash, '#')
        && brackets.delimiter() == Delimiter::Bracket
    {
        rest = more;
    }
    if let [TokenTree::Ident(visibility), more @ ..] = rest
        && ident_name(visibility) == "pub"
    {
        rest = match more {
            [TokenTree::Group(scope), most @ ..] if scope.delimiter() == Delimiter::Parenthesis => {
                most
            }
            _ => more,
        };
    }
    matches!(
        rest,
        [TokenTree::Ident(keyword), TokenTree::Ident(_), semi, ..]
            if ident_name(keyword) == "mod" && is_punct(semi, ';')
    )
}

/// The repo-relative file that `value`, a `path` attribute's value in `file`, names: resolved
/// against `file`'s folder one component at a time. `None` when `value` is empty, absolute, holds
/// a `\` or a `:`, or leaves `dir` at any step.
pub(super) fn resolve(value: &str, file: &str, dir: &str) -> Option<String> {
    if value.is_empty() || value.starts_with('/') || value.contains(['\\', ':']) {
        return None;
    }
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name),
        }
        if !workspace::is_under(&parts.join("/"), dir) {
            return None;
        }
    }
    Some(parts.join("/"))
}
