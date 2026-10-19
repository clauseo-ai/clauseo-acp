//! Macro tokens for the core-io check. `syn` leaves the arguments of a macro call and the rules
//! of a `macro_rules!` as tokens. `bodies` parses them as items, statements or expressions when
//! it can, so they are read like other code; `chains` walks the tokens that do not parse (a rule
//! with metavariables, a macro of its own syntax) for `a::b::c` paths.

use super::names::{self, Written};
use proc_macro2::{Delimiter, Spacing, Span, TokenStream, TokenTree};
use syn::parse::Parser;
use syn::punctuated::Punctuated;

/// The code of one macro call or of one `macro_rules!` rule.
pub(super) enum Body {
    Items(Vec<syn::Item>),
    Stmts(Vec<syn::Stmt>),
    /// Comma-separated expressions, as in `vec![a, b]` or `format!("{}", a)`.
    Exprs(Vec<syn::Expr>),
    /// Tokens that parse as none of those.
    Tokens(TokenStream),
}

/// The code of `mac`: one body per rule of a `macro_rules!` (its matchers skipped), else one.
pub(super) fn bodies(mac: &syn::Macro) -> Vec<Body> {
    if mac.path.is_ident("macro_rules")
        && let Some(rules) = rule_bodies(mac.tokens.clone())
    {
        return rules.into_iter().map(parse).collect();
    }
    vec![parse(mac.tokens.clone())]
}

/// The expansion of each rule `(matcher) => { expansion };`, or `None` for another shape.
fn rule_bodies(tokens: TokenStream) -> Option<Vec<TokenStream>> {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut bodies = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let matcher = matches!(tokens.get(index), Some(TokenTree::Group(_)));
        let arrow = matches!(
            tokens.get(index + 1),
            Some(TokenTree::Punct(eq)) if eq.as_char() == '=' && eq.spacing() == Spacing::Joint
        ) && tokens.get(index + 2).is_some_and(|gt| is_punct(gt, '>'));
        let Some(TokenTree::Group(body)) = tokens.get(index + 3) else {
            return None;
        };
        if !matcher || !arrow || body.delimiter() == Delimiter::None {
            return None;
        }
        bodies.push(body.stream());
        index += 4;
        if tokens.get(index).is_some_and(|semi| is_punct(semi, ';')) {
            index += 1;
        }
    }
    Some(bodies)
}

/// `tokens` as items, else statements, else comma-separated expressions, else tokens.
fn parse(tokens: TokenStream) -> Body {
    if let Ok(file) = syn::parse2::<syn::File>(tokens.clone()) {
        return Body::Items(file.items);
    }
    if let Ok(stmts) = syn::Block::parse_within.parse2(tokens.clone()) {
        return Body::Stmts(stmts);
    }
    let exprs = Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    if let Ok(exprs) = exprs.parse2(tokens.clone()) {
        return Body::Exprs(exprs.into_iter().collect());
    }
    Body::Tokens(tokens)
}

/// How a chain of tokens is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Kind {
    /// `a::b::c`, `::a::b` or `<a::b>::c`.
    Path,
    /// `a::b::$x`: the path, continued by the metavariable `x`.
    ThenMetavariable(String),
    /// `$x::a::b` or `<$x>::a::b`: the segments after the metavariable `x`.
    Metavariable(String),
    /// `type A = $t` or `use $p`: a binding to whatever the metavariable expands to.
    Binding(String),
}

/// One chain of a token stream: its segments, the span of its start and how it is written.
#[derive(Debug, Clone)]
pub(super) struct Chain {
    pub path: Written,
    pub span: Span,
    pub kind: Kind,
}

/// The chains of a token stream and its groups.
pub(super) fn chains(tokens: TokenStream) -> Vec<Chain> {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        index = match token {
            TokenTree::Group(group) => {
                found.extend(chains(group.stream()));
                index + 1
            }
            TokenTree::Ident(ident) => match binding(&tokens, index) {
                Some((written, end)) => {
                    let span = ident.span();
                    let path = Written {
                        segments: Vec::new(),
                        leading_colon: false,
                    };
                    found.push(Chain {
                        path,
                        span,
                        kind: Kind::Binding(written),
                    });
                    end
                }
                None => path_chain(&tokens, index, false, &mut found),
            },
            TokenTree::Punct(_) if is_path_sep(&tokens, index) => {
                if matches!(tokens.get(index + 2), Some(TokenTree::Ident(_))) {
                    path_chain(&tokens, index + 2, true, &mut found)
                } else {
                    index + 2
                }
            }
            TokenTree::Punct(punct) => metavariable_chain(&tokens, index, punct.span(), &mut found),
            TokenTree::Literal(_) => index + 1,
        };
    }
    found
}

/// `type A = $t` or `use $p` (also `use ::$p`) at `index`: how it is written and the index after
/// the metavariable.
fn binding(tokens: &[TokenTree], index: usize) -> Option<(String, usize)> {
    let Some(TokenTree::Ident(keyword)) = tokens.get(index) else {
        return None;
    };
    if keyword == "type" {
        let Some(TokenTree::Ident(alias)) = tokens.get(index + 1) else {
            return None;
        };
        let equals = tokens.get(index + 2).is_some_and(|eq| is_punct(eq, '='));
        let name = metavariable_at(tokens, index + 3).filter(|_| equals)?;
        Some((
            format!("type {} = ${name}", names::name_of(alias)),
            index + 5,
        ))
    } else if keyword == "use" {
        let start = if is_path_sep(tokens, index + 1) {
            index + 3
        } else {
            index + 1
        };
        let name = metavariable_at(tokens, start)?;
        Some((format!("use ${name}"), start + 2))
    } else {
        None
    }
}

/// Reads the path chain whose first identifier is at `start`; returns the index after it.
fn path_chain(tokens: &[TokenTree], start: usize, leading: bool, found: &mut Vec<Chain>) -> usize {
    let Some(TokenTree::Ident(ident)) = tokens.get(start) else {
        return start + 1;
    };
    let mut path = Written {
        segments: vec![names::name_of(ident)],
        leading_colon: leading,
    };
    let mut span = if leading {
        tokens.get(start - 2).map_or(ident.span(), TokenTree::span)
    } else {
        ident.span()
    };
    let mut end = follow(tokens, start + 1, &mut path.segments);
    let opened = !leading
        && start
            .checked_sub(1)
            .and_then(|before| tokens.get(before))
            .is_some_and(|before| is_punct(before, '<'));
    if opened
        && tokens.get(end).is_some_and(|after| is_punct(after, '>'))
        && is_path_sep(tokens, end + 1)
    {
        span = tokens.get(start - 1).map_or(span, TokenTree::span);
        end = follow(tokens, end + 1, &mut path.segments);
    }
    let kind = match metavariable_at(tokens, end + 2) {
        Some(name) if is_path_sep(tokens, end) => {
            end += 4;
            Kind::ThenMetavariable(name)
        }
        _ => Kind::Path,
    };
    found.push(Chain { path, span, kind });
    end
}

/// Reads `$x::a::b`, `<$x>::a::b` or `$crate::a` at the punctuation at `index`; returns the
/// index after what it read.
fn metavariable_chain(
    tokens: &[TokenTree],
    index: usize,
    span: Span,
    found: &mut Vec<Chain>,
) -> usize {
    let (name, after) = match tokens.get(index) {
        Some(token) if is_punct(token, '$') => match metavariable_at(tokens, index) {
            Some(name) => (name, index + 2),
            None => return index + 1,
        },
        Some(token) if is_punct(token, '<') => match metavariable_at(tokens, index + 1) {
            Some(name) if tokens.get(index + 3).is_some_and(|gt| is_punct(gt, '>')) => {
                (name, index + 4)
            }
            _ => return index + 1,
        },
        _ => return index + 1,
    };
    let mut rest = Vec::new();
    let end = follow(tokens, after, &mut rest);
    if !rest.is_empty() {
        found.push(Chain {
            path: Written {
                segments: rest,
                leading_colon: false,
            },
            span,
            kind: Kind::Metavariable(name),
        });
    }
    end
}

/// The name of the metavariable `$name` at `index`; `None` for `$crate` and anything else.
fn metavariable_at(tokens: &[TokenTree], index: usize) -> Option<String> {
    match (tokens.get(index), tokens.get(index + 1)) {
        (Some(dollar), Some(TokenTree::Ident(ident)))
            if is_punct(dollar, '$') && ident != "crate" =>
        {
            Some(names::name_of(ident))
        }
        _ => None,
    }
}

/// Appends each `:: ident` from `index` on to `segments`; returns the index after the last.
fn follow(tokens: &[TokenTree], mut index: usize, segments: &mut Vec<String>) -> usize {
    while is_path_sep(tokens, index) {
        let Some(TokenTree::Ident(ident)) = tokens.get(index + 2) else {
            break;
        };
        segments.push(names::name_of(ident));
        index += 3;
    }
    index
}

/// True when the tokens at `index` are `::`: a joint `:` and another `:`.
fn is_path_sep(tokens: &[TokenTree], index: usize) -> bool {
    matches!(
        tokens.get(index),
        Some(TokenTree::Punct(first)) if first.as_char() == ':' && first.spacing() == Spacing::Joint
    ) && tokens
        .get(index + 1)
        .is_some_and(|second| is_punct(second, ':'))
}

fn is_punct(token: &TokenTree, c: char) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == c)
}
