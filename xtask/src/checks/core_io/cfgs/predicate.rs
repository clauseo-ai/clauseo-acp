//! The predicate of a `cfg` or `cfg_attr` in a held crate: built only from `all`, `any`, `not`,
//! `test`, `doc` and `feature = "<literal>"`, with no `feature` under a `not`.

use super::{Finding, ident_name, is_punct, line_of};
use proc_macro2::{Delimiter, Group, TokenTree};

/// `Ok` when `trees`, the contents of `group`, are one allowed predicate, with an optional
/// trailing comma; else the first `Finding` and its line.
pub(super) fn whole_predicate(trees: &[TokenTree], group: &Group) -> Result<(), (Finding, u32)> {
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
        ("test" | "doc", []) => Ok(()),
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
        ("feature" | "all" | "any" | "not" | "test" | "doc", _) => Err(not_a_predicate(line)),
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

/// The finding for a metavariable in place of a predicate.
pub(super) fn metavariable() -> Finding {
    Finding::Predicate("a metavariable".to_owned())
}
