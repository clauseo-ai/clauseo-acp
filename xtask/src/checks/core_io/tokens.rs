//! Macro tokens for the core-io check. `syn` leaves the arguments of a macro call and the rules
//! of a `macro_rules!` as tokens. `bodies` parses them as items, statements or expressions when
//! it can, so they are read like other code. In a `macro_rules!`, each metavariable `$x` first
//! becomes a placeholder identifier (see `is_placeholder`; one before an item keyword, which can
//! only be a visibility, is dropped), `$crate` becomes `crate`, and each
//! repetition `$( ... )` with its separator and `*`, `+` or `?` becomes its inner tokens, once,
//! so most rule bodies parse. `chains` walks the tokens that still do not parse (a macro of its
//! own syntax, the arguments of an attribute) for `a::b::c` paths.

use super::names::{self, Written};
use proc_macro2::{Delimiter, Group, Ident, Spacing, Span, TokenStream, TokenTree};
use syn::parse::Parser;
use syn::punctuated::Punctuated;

/// How a placeholder starts; the metavariable's name follows. Nobody writes such a name, so a
/// path segment that starts with it stands for whatever the metavariable expands to.
const PLACEHOLDER: &str = "__core_io_metavariable__";

/// The keywords that can start an item after its visibility.
const ITEM_KEYWORDS: [&str; 14] = [
    "async", "const", "enum", "extern", "fn", "impl", "mod", "static", "struct", "trait", "type",
    "union", "unsafe", "use",
];

/// True when `segment` is the placeholder of a metavariable.
pub(super) fn is_placeholder(segment: &str) -> bool {
    segment.starts_with(PLACEHOLDER)
}

/// `segment` as the macro writes it: `$x` for the placeholder of `$x`, else itself.
pub(super) fn render(segment: &str) -> String {
    match segment.strip_prefix(PLACEHOLDER) {
        Some(name) => format!("${name}"),
        None => segment.to_owned(),
    }
}

/// `path` joined with `::`, each placeholder written as its metavariable.
pub(super) fn join(path: &[String]) -> String {
    let parts: Vec<String> = path.iter().map(|seg| render(seg)).collect();
    parts.join("::")
}

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
    if !mac.path.is_ident("macro_rules") {
        return vec![parse(mac.tokens.clone())];
    }
    let tokens = substitute(mac.tokens.clone());
    match rule_bodies(tokens.clone()) {
        Some(rules) => rules.into_iter().map(parse).collect(),
        None => vec![parse(tokens)],
    }
}

/// `tokens` with each metavariable replaced by its placeholder, `$crate` by `crate`, and each
/// repetition by its inner tokens.
fn substitute(tokens: TokenStream) -> TokenStream {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::new();
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        index += 1;
        match (token, tokens.get(index)) {
            (TokenTree::Group(group), _) => {
                let mut inner = Group::new(group.delimiter(), substitute(group.stream()));
                inner.set_span(group.span());
                out.push(TokenTree::Group(inner));
            }
            (TokenTree::Punct(dollar), Some(TokenTree::Ident(ident)))
                if dollar.as_char() == '$' =>
            {
                let name = names::name_of(ident);
                let text = if name == "crate" {
                    name
                } else {
                    format!("{PLACEHOLDER}{name}")
                };
                // Before an item keyword the metavariable can only be a visibility (`$vis fn`),
                // which names no path; dropping it lets the item parse.
                let visibility = matches!(
                    tokens.get(index + 1),
                    Some(TokenTree::Ident(next)) if ITEM_KEYWORDS.iter().any(|word| next == word)
                );
                if !visibility {
                    out.push(TokenTree::Ident(Ident::new(&text, ident.span())));
                }
                index += 1;
            }
            (TokenTree::Punct(dollar), Some(TokenTree::Group(group)))
                if dollar.as_char() == '$' && group.delimiter() == Delimiter::Parenthesis =>
            {
                match repetition_end(&tokens, index + 1) {
                    Some(end) => {
                        out.extend(substitute(group.stream()));
                        index = end;
                    }
                    None => out.push(token.clone()),
                }
            }
            _ => out.push(token.clone()),
        }
    }
    out.into_iter().collect()
}

/// The index after the `*`, `+` or `?` that ends a repetition whose group ends before `at`,
/// past a separator of one token (`,`, `;`, `=>`) when there is one; `None` for another shape.
fn repetition_end(tokens: &[TokenTree], at: usize) -> Option<usize> {
    let op = |index: usize, ops: &str| {
        tokens
            .get(index)
            .is_some_and(|token| ops.chars().any(|op| is_punct(token, op)))
    };
    if op(at, "*+?") {
        return Some(at + 1);
    }
    // A separator is one token; `=>` and the like are joined punctuation.
    for len in 1..=3 {
        let sep = tokens.get(at..at + len)?;
        let one = len == 1 && !matches!(sep.first(), Some(TokenTree::Group(_)));
        let joined = sep.iter().all(|token| matches!(token, TokenTree::Punct(_)));
        if (one || joined) && op(at + len, "*+") {
            return Some(at + len + 1);
        }
    }
    None
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

/// One `a::b::c`, `::a::b` or `<a::b>::c` chain of a token stream, and the span of its start.
#[derive(Debug, Clone)]
pub(super) struct Chain {
    pub path: Written,
    pub span: Span,
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
            TokenTree::Ident(_) => path_chain(&tokens, index, false, &mut found),
            TokenTree::Punct(_)
                if is_path_sep(&tokens, index)
                    && matches!(tokens.get(index + 2), Some(TokenTree::Ident(_))) =>
            {
                path_chain(&tokens, index + 2, true, &mut found)
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => index + 1,
        };
    }
    found
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
    found.push(Chain { path, span });
    end
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
