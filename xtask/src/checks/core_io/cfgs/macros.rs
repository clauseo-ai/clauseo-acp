//! The arms of a `macro_rules!` definition. The metavariable rules skip the matcher of each arm,
//! which only matches tokens and builds nothing. In the transcriber, `#[$m]` passes when the same
//! arm's matcher binds `$m` as `#[$m:meta]`, since a caller then writes a whole bracketed
//! attribute, which the walk checks where the caller writes it. Both hold only for a definition
//! outside every transcriber and every macro call's arguments. There another macro can fill in
//! the matcher and the `$m`, or the tokens are no definition at all, so every rule applies.

use super::{Finding, NO_NAMES, Scope, group, ident_name, is_punct, walk};
use proc_macro2::{Group, TokenStream, TokenTree};
use std::collections::{BTreeMap, BTreeSet};

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
/// `scope` reads no definitions is walked in `scope` with every metavariable rule and no
/// forwarded attribute, and a body of
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
        let binds = bindings(matcher.stream());
        let transcribing = Scope {
            metas: &metas,
            binds: Some(&binds),
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

/// The fragment kind of each metavariable that `tokens`, a matcher, binds as `$name:kind`, inside
/// repetitions too.
fn bindings(tokens: TokenStream) -> BTreeMap<String, String> {
    let mut kinds = BTreeMap::new();
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        match (tree, &trees[index + 1..]) {
            (TokenTree::Group(inner), _) => kinds.extend(bindings(inner.stream())),
            (dollar, [TokenTree::Ident(name), colon, TokenTree::Ident(kind), ..])
                if is_punct(dollar, '$') && is_punct(colon, ':') =>
            {
                kinds.insert(ident_name(name), ident_name(kind));
            }
            _ => {}
        }
    }
    kinds
}

/// True when `before`, the trees ahead of a bracket group in a transcriber whose arm binds
/// `binds`, end in a repetition such as `$($h)*`, or in a metavariable other than `$crate` that
/// the arm binds as `tt` or does not bind. Either can stand for a `#` that turns the brackets into
/// an attribute.
pub(super) fn joins_brackets(before: &[TokenTree], binds: &BTreeMap<String, String>) -> bool {
    let is_op =
        |tree: &TokenTree| is_punct(tree, '*') || is_punct(tree, '+') || is_punct(tree, '?');
    match before {
        [.., dollar, TokenTree::Ident(name)] if is_punct(dollar, '$') => {
            let name = ident_name(name);
            name != "crate" && binds.get(&name).is_none_or(|kind| kind == "tt")
        }
        [.., dollar, TokenTree::Group(_), op] if is_punct(dollar, '$') && is_op(op) => true,
        [.., dollar, TokenTree::Group(_), _, op] => is_punct(dollar, '$') && is_op(op),
        _ => false,
    }
}
