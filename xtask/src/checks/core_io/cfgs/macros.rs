//! The arms of a `macro_rules!` definition, as the walk in `cfgs.rs` reads them. Where the walk's
//! scope reads definitions, `arms` walks each matcher in a matcher scope and each transcriber with
//! the names its own matcher binds as `#[$m:meta]`. Elsewhere it walks the body as it finds it.

use super::{Finding, NO_NAMES, Scope, group, is_punct, walk};
use proc_macro2::{Group, TokenStream, TokenTree};
use std::collections::BTreeSet;

/// True when `before`, the trees ahead of a group, ends in `macro_rules! name`, so the group
/// holds the arms of a definition.
pub(super) fn holds_arms(before: &[TokenTree]) -> bool {
    matches!(
        before,
        [.., TokenTree::Ident(rules), bang, TokenTree::Ident(_)]
            if super::ident_name(rules) == "macro_rules" && is_punct(bang, '!')
    )
}

/// Pushes onto `hits` each `Finding` in the arms of the definition `body`, met in `scope`. Each
/// arm is a matcher group, `=>` and a transcriber group, with a `;` between arms. A body where
/// `scope` reads no definitions is walked in `scope` with no forwarded attribute, and a body of
/// any other shape is walked as if it were outside any macro.
pub(super) fn arms(body: &Group, scope: Scope<'_>, hits: &mut Vec<(Finding, u32)>) {
    if !scope.definitions {
        let full = Scope {
            metas: &NO_NAMES,
            top: None,
            ..scope
        };
        walk(body.stream(), full, hits);
        return;
    }
    let trees: Vec<TokenTree> = body.stream().into_iter().collect();
    let Some(arms) = split_arms(&trees) else {
        walk(body.stream(), Scope::outside(), hits);
        return;
    };
    for (matcher, transcriber) in arms {
        let matching = Scope {
            matcher: true,
            ..Scope::outside()
        };
        group(matcher, matching, false, hits);
        let metas = meta_bindings(matcher.stream());
        let transcribing = Scope {
            metas: &metas,
            ..Scope::outside()
        };
        group(transcriber, transcribing, false, hits);
    }
}

/// The (matcher, transcriber) pairs of `trees`, or `None` when they are not a list of arms.
fn split_arms(trees: &[TokenTree]) -> Option<Vec<(&Group, &Group)>> {
    let mut arms = Vec::new();
    let mut rest = trees;
    while !rest.is_empty() {
        let [
            TokenTree::Group(matcher),
            eq,
            gt,
            TokenTree::Group(transcriber),
            after @ ..,
        ] = rest
        else {
            return None;
        };
        if !is_punct(eq, '=') || !is_punct(gt, '>') {
            return None;
        }
        arms.push((matcher, transcriber));
        rest = match after {
            [semi, more @ ..] if is_punct(semi, ';') => more,
            [] => after,
            _ => return None,
        };
    }
    Some(arms)
}

/// The names that `tokens`, a matcher, binds as `#[$m:meta]`, inside repetitions too.
fn meta_bindings(tokens: TokenStream) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        let TokenTree::Group(inner) = tree else {
            continue;
        };
        let after_hash = index > 0 && is_punct(&trees[index - 1], '#');
        let parts: Vec<TokenTree> = inner.stream().into_iter().collect();
        match parts.as_slice() {
            [
                dollar,
                TokenTree::Ident(name),
                colon,
                TokenTree::Ident(kind),
            ] if after_hash && is_punct(dollar, '$') && is_punct(colon, ':') && kind == "meta" => {
                names.insert(name.to_string());
            }
            _ => names.extend(meta_bindings(inner.stream())),
        }
    }
    names
}

/// True when `parts`, the inside of a bracket group after a `#`, are exactly `$m` for a name
/// `m` in `scope.metas`.
pub(super) fn forwards_meta(parts: &[TokenTree], scope: Scope<'_>) -> bool {
    matches!(
        parts,
        [dollar, TokenTree::Ident(name)]
            if is_punct(dollar, '$') && scope.metas.contains(&name.to_string())
    )
}
