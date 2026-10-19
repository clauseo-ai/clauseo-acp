//! Macro tokens for the core-io check. `syn` leaves the arguments of a macro call and the rules
//! of a `macro_rules!` as tokens. `bodies` parses them as items, statements, expressions, a
//! type, a pattern, match arms or named fields, so they are read like other code.
//!
//! In a `macro_rules!` rule, each metavariable `$x` of the rule's matcher is first replaced by
//! something of its fragment kind that parses where it stands: `{}` for a `block`, a placeholder
//! lifetime for a `lifetime`, `0` for a `literal`, nothing for a `vis` or an `item`, and a
//! placeholder identifier for the rest (see `is_placeholder`). `$crate` becomes `crate`. Each
//! repetition `$( ... )` with its separator and `*`, `+` or `?` becomes its inner tokens, twice
//! when its separator or its edges are `::`, so a path it builds still has a placeholder segment
//! after the first one. A rule body that parses as none of those is `Body::Unreadable`, and the
//! check fails it; a nested `macro_rules!` that escapes its own `$` with `$$` (an unstable
//! feature) is one such body. `chains` walks what remains as tokens for `a::b::c` paths: the
//! arguments of a macro call that parse as no code (a macro of its own syntax) and the arguments
//! of attributes.

use super::names::{self, Written};
use proc_macro2::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use std::collections::BTreeMap;
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::visit::Visit;

/// How a placeholder starts; the metavariable's name follows. Nobody writes such a name, so a
/// path segment that starts with it stands for whatever the metavariable expands to.
const PLACEHOLDER: &str = "__core_io_metavariable__";

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
    Type(Box<syn::Type>),
    /// A pattern, as in `Some(ref $x)`.
    Pat(Box<syn::Pat>),
    /// Match arms, as in `$($p => $e),*`.
    Arms(Vec<syn::Arm>),
    /// Named struct fields, as in `$($n: $t),*`.
    Fields(Vec<syn::Field>),
    /// The arguments of a macro call that parse as none of those.
    Tokens(TokenStream),
    /// A `macro_rules!` rule whose body parses as none of those, or a `macro_rules!` whose
    /// rules have another shape; the span of its opening delimiter.
    Unreadable(Span),
}

impl Body {
    /// Visits the code of the body with `visitor`; nothing for tokens or an unreadable rule.
    pub(super) fn visit<'a>(&'a self, visitor: &mut impl Visit<'a>) {
        match self {
            Body::Items(items) => items.iter().for_each(|item| visitor.visit_item(item)),
            Body::Stmts(stmts) => stmts.iter().for_each(|stmt| visitor.visit_stmt(stmt)),
            Body::Exprs(exprs) => exprs.iter().for_each(|expr| visitor.visit_expr(expr)),
            Body::Type(ty) => visitor.visit_type(ty),
            Body::Pat(pat) => visitor.visit_pat(pat),
            Body::Arms(arms) => arms.iter().for_each(|arm| visitor.visit_arm(arm)),
            Body::Fields(fields) => fields.iter().for_each(|field| visitor.visit_field(field)),
            Body::Tokens(_) | Body::Unreadable(_) => {}
        }
    }
}

/// The code of `mac`: one body per rule of a `macro_rules!` (its matchers skipped), else one.
pub(super) fn bodies(mac: &syn::Macro) -> Vec<Body> {
    if !mac.path.is_ident("macro_rules") {
        let tokens = mac.tokens.clone();
        return vec![parse(tokens.clone()).unwrap_or(Body::Tokens(tokens))];
    }
    let Some(rules) = rules(mac.tokens.clone()) else {
        return vec![Body::Unreadable(mac.delimiter.span().open())];
    };
    rules
        .into_iter()
        .map(|(matcher, body)| {
            let mut kinds = BTreeMap::new();
            fragments(matcher.stream(), &mut kinds);
            parse(substitute(body.stream(), &kinds))
                .unwrap_or_else(|| Body::Unreadable(body.span_open()))
        })
        .collect()
}

/// Records the fragment kind of each `$name:kind` in `matcher`, its repetitions included.
fn fragments(matcher: TokenStream, kinds: &mut BTreeMap<String, String>) {
    let tokens: Vec<TokenTree> = matcher.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        match (token, tokens.get(index + 1..index + 4)) {
            (TokenTree::Group(group), _) => fragments(group.stream(), kinds),
            (
                TokenTree::Punct(dollar),
                Some(
                    [
                        TokenTree::Ident(name),
                        TokenTree::Punct(colon),
                        TokenTree::Ident(kind),
                    ],
                ),
            ) if dollar.as_char() == '$' && colon.as_char() == ':' => {
                kinds.insert(names::name_of(name), kind.to_string());
            }
            _ => {}
        }
    }
}

/// `tokens` with each metavariable in `kinds` replaced by what parses in its place (see the
/// module docs), `$crate` by `crate`, and each repetition by its inner tokens. A `$name` that
/// the matcher does not bind stays, as in the body of a `macro_rules!` written inside the rule.
fn substitute(tokens: TokenStream, kinds: &BTreeMap<String, String>) -> TokenStream {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::new();
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        index += 1;
        match (token, tokens.get(index)) {
            (TokenTree::Group(group), _) => {
                let mut inner = Group::new(group.delimiter(), substitute(group.stream(), kinds));
                inner.set_span(group.span());
                out.push(TokenTree::Group(inner));
            }
            (TokenTree::Punct(dollar), Some(TokenTree::Ident(ident)))
                if dollar.as_char() == '$' && ident == "crate" =>
            {
                out.push(TokenTree::Ident(Ident::new("crate", ident.span())));
                index += 1;
            }
            (TokenTree::Punct(dollar), Some(TokenTree::Ident(ident)))
                if dollar.as_char() == '$' =>
            {
                let name = names::name_of(ident);
                match kinds.get(&name) {
                    Some(kind) => {
                        out.extend(fragment(kind, &name, ident.span()));
                        index += 1;
                    }
                    None => out.push(token.clone()),
                }
            }
            (TokenTree::Punct(dollar), Some(TokenTree::Group(group)))
                if dollar.as_char() == '$' && group.delimiter() == Delimiter::Parenthesis =>
            {
                match repetition_end(&tokens, index + 1) {
                    Some(end) => {
                        let inner: Vec<TokenTree> =
                            substitute(group.stream(), kinds).into_iter().collect();
                        let separator = tokens.get(index + 1..end - 1).unwrap_or_default();
                        let path_sep = is_path_sep(separator, 0);
                        out.extend(inner.iter().cloned());
                        if path_sep || builds_a_path(&inner) {
                            if path_sep {
                                out.extend(separator.iter().cloned());
                            }
                            out.extend(inner);
                        }
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

/// What stands in for the metavariable `name` of fragment kind `kind`, with `span`.
fn fragment(kind: &str, name: &str, span: Span) -> Vec<TokenTree> {
    let placeholder = Ident::new(&format!("{PLACEHOLDER}{name}"), span);
    match kind {
        "block" => {
            let mut block = Group::new(Delimiter::Brace, TokenStream::new());
            block.set_span(span);
            vec![TokenTree::Group(block)]
        }
        "lifetime" => {
            let mut quote = Punct::new('\'', Spacing::Joint);
            quote.set_span(span);
            vec![TokenTree::Punct(quote), TokenTree::Ident(placeholder)]
        }
        "literal" => {
            let mut zero = Literal::u8_unsuffixed(0);
            zero.set_span(span);
            vec![TokenTree::Literal(zero)]
        }
        "vis" | "item" => Vec::new(),
        _ => vec![TokenTree::Ident(placeholder)],
    }
}

/// True when `tokens` start or end with `::`, so repeating them builds a longer path.
fn builds_a_path(tokens: &[TokenTree]) -> bool {
    is_path_sep(tokens, 0) || tokens.len() >= 2 && is_path_sep(tokens, tokens.len() - 2)
}

/// The index after the `*`, `+` or `?` that ends a repetition whose group ends before `at`,
/// past a separator of one token (`,`, `;`, `=>`, `::`) when there is one; `None` for another
/// shape.
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

/// Each rule `(matcher) => { expansion };`, or `None` for another shape.
fn rules(tokens: TokenStream) -> Option<Vec<(Group, Group)>> {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut rules = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let Some(TokenTree::Group(matcher)) = tokens.get(index) else {
            return None;
        };
        let arrow = matches!(
            tokens.get(index + 1),
            Some(TokenTree::Punct(eq)) if eq.as_char() == '=' && eq.spacing() == Spacing::Joint
        ) && tokens.get(index + 2).is_some_and(|gt| is_punct(gt, '>'));
        let Some(TokenTree::Group(body)) = tokens.get(index + 3) else {
            return None;
        };
        if !arrow || body.delimiter() == Delimiter::None {
            return None;
        }
        rules.push((matcher.clone(), body.clone()));
        index += 4;
        if tokens.get(index).is_some_and(|semi| is_punct(semi, ';')) {
            index += 1;
        }
    }
    Some(rules)
}

/// `tokens` as items, else statements, comma-separated expressions, a type, a pattern, match
/// arms or named fields, the first that parses.
fn parse(tokens: TokenStream) -> Option<Body> {
    if let Ok(file) = syn::parse2::<syn::File>(tokens.clone()) {
        return Some(Body::Items(file.items));
    }
    if let Ok(stmts) = syn::Block::parse_within.parse2(tokens.clone()) {
        return Some(Body::Stmts(stmts));
    }
    let exprs = Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    if let Ok(exprs) = exprs.parse2(tokens.clone()) {
        return Some(Body::Exprs(exprs.into_iter().collect()));
    }
    if let Ok(ty) = syn::parse2::<syn::Type>(tokens.clone()) {
        return Some(Body::Type(Box::new(ty)));
    }
    if let Ok(pat) = syn::Pat::parse_multi_with_leading_vert.parse2(tokens.clone()) {
        return Some(Body::Pat(Box::new(pat)));
    }
    let arms = |input: ParseStream<'_>| {
        let mut arms = Vec::new();
        while !input.is_empty() {
            arms.push(input.parse::<syn::Arm>()?);
        }
        Ok(arms)
    };
    if let Ok(arms) = arms.parse2(tokens.clone()) {
        return Some(Body::Arms(arms));
    }
    let fields = |input: ParseStream<'_>| {
        Punctuated::<syn::Field, syn::Token![,]>::parse_terminated_with(
            input,
            syn::Field::parse_named,
        )
    };
    fields
        .parse2(tokens)
        .ok()
        .map(|fields| Body::Fields(fields.into_iter().collect()))
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
