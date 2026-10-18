//! The second pass of the core-io check: every place a parsed file reaches a banned path.
//!
//! It visits every `use` leaf, every path (in expressions, patterns, types, attributes and
//! macro names) and every qualified path `<T>::f`, and resolves each through the crate's
//! `Table`. `syn` leaves macro arguments and `macro_rules!` bodies as tokens, so those (and the
//! tokens of attribute lists) are walked for `a::b::c` chains, a leading `::` and a `<T>::f`
//! included, which resolve the same way. A `use` inside a macro body binds nothing.

use super::names::{self, Leaf, Module, Table, Target, Written};
use proc_macro2::{Spacing, Span, TokenStream, TokenTree};
use std::collections::BTreeSet;
use syn::visit::{self, Visit};

/// The banned modules: any path inside `std::<module>`, or naming it.
const MODULES: [&str; 4] = ["fs", "net", "process", "env"];

/// The types whose `now` is banned: `std::time::<clock>::now`.
const CLOCKS: [&str; 2] = ["Instant", "SystemTime"];

/// What a site reaches.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Reach {
    /// A path inside a banned module or a banned function: `std::fs`, `std::time::Instant::now`.
    Uses(String),
    /// A glob import of `prefix` (`std::time`), which reaches these banned paths.
    Glob {
        prefix: String,
        reaches: Vec<String>,
    },
}

/// One offending site in a file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Site {
    /// 1-based.
    pub line: u32,
    /// 0-based, in characters; only orders sites and tells two on one line apart.
    pub column: usize,
    pub reach: Reach,
}

/// The banned path `path` (a path inside `std`) is in or is, if any: `std::fs` for
/// `std::fs::read`, `std::time::Instant::now` for itself.
fn banned(path: &[String]) -> Option<String> {
    match path {
        [std, module, ..] if std == "std" && MODULES.contains(&module.as_str()) => {
            Some(format!("std::{module}"))
        }
        [std, time, clock, now, ..]
            if std == "std"
                && time == "time"
                && now == "now"
                && CLOCKS.contains(&clock.as_str()) =>
        {
            Some(format!("std::time::{clock}::now"))
        }
        _ => None,
    }
}

/// The banned paths a glob import of `prefix` reaches: the one it sits inside, else every banned
/// path that starts with `prefix`.
fn glob_reaches(prefix: &[String]) -> Vec<String> {
    if let Some(inside) = banned(prefix) {
        return vec![inside];
    }
    let modules = MODULES.iter().map(|module| vec!["std", module]);
    let clocks = CLOCKS.iter().map(|clock| vec!["std", "time", clock, "now"]);
    modules
        .chain(clocks)
        .filter(|path| {
            path.len() > prefix.len() && path.iter().zip(prefix).all(|(part, seg)| part == seg)
        })
        .map(|path| path.join("::"))
        .collect()
}

/// Every offending site in `file`, whose module is `module`, sorted by position.
pub(super) fn sites(table: &Table, module: &[String], file: &syn::File) -> Vec<Site> {
    let mut finder = Finder {
        table,
        module: module.to_vec(),
        found: BTreeSet::new(),
    };
    finder.visit_file(file);
    finder.found.into_iter().collect()
}

/// The second pass: finds the sites of one file.
struct Finder<'t> {
    table: &'t Table,
    module: Module,
    found: BTreeSet<Site>,
}

impl Finder<'_> {
    /// The first banned path `path` may reach, if any.
    fn reach(&self, path: &Written) -> Option<String> {
        self.table
            .resolve(&self.module, path)
            .into_iter()
            .find_map(|target| match target {
                Target::Std(found) => banned(&found),
                Target::Crate(_) => None,
            })
    }

    /// Adds a site at `span` when `path` may reach a banned path.
    fn check(&mut self, path: &Written, span: Span) {
        if let Some(reach) = self.reach(path) {
            self.add(span, Reach::Uses(reach));
        }
    }

    fn add(&mut self, span: Span, reach: Reach) {
        let start = span.start();
        self.found.insert(Site {
            line: u32::try_from(start.line).unwrap_or(u32::MAX),
            column: start.column,
            reach,
        });
    }

    /// Checks a qualified path `<T>::rest` as the path `T::rest`, at the `<`. When `T` itself is
    /// banned (`<std::fs::File>::open`), the site is `T`'s, found when `T` is visited.
    fn check_qualified(&mut self, qself: &syn::QSelf, path: &syn::Path) {
        if let syn::Type::Path(ty) = &*qself.ty
            && ty.qself.is_none()
            && self.reach(&names::written(&ty.path)).is_none()
        {
            let mut joined = names::written(&ty.path);
            joined.segments.extend(names::written(path).segments);
            self.check(&joined, qself.lt_token.spans[0]);
        }
    }

    /// Checks the `a::b` chains in a token stream (see the module docs).
    fn check_tokens(&mut self, tokens: TokenStream) {
        for (path, span) in chains(tokens) {
            self.check(&path, span);
        }
    }

    /// A path whose qualified self is `<T>` with no `as`: check it whole, and visit the rest
    /// without checking the bare tail (`::now`) on its own.
    fn visit_qualified(&mut self, qself: &syn::QSelf, path: &syn::Path) {
        self.check_qualified(qself, path);
        self.visit_qself(qself);
        for segment in &path.segments {
            self.visit_path_arguments(&segment.arguments);
        }
    }
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        self.module.push(i.ident.to_string());
        visit::visit_item_mod(self, i);
        self.module.pop();
    }

    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        for attr in &i.attrs {
            self.visit_attribute(attr);
        }
        for leaf in names::use_leaves(i) {
            match leaf {
                Leaf::Bind { path, span, .. } => self.check(&path, span),
                Leaf::Glob { prefix, span } => {
                    let reach = self
                        .table
                        .resolve(&self.module, &prefix)
                        .into_iter()
                        .find_map(|target| {
                            let Target::Std(found) = target else {
                                return None;
                            };
                            let reaches = glob_reaches(&found);
                            (!reaches.is_empty()).then(|| Reach::Glob {
                                prefix: found.join("::"),
                                reaches,
                            })
                        });
                    if let Some(reach) = reach {
                        self.add(span, reach);
                    }
                }
            }
        }
    }

    fn visit_path(&mut self, i: &'ast syn::Path) {
        let span = match (&i.leading_colon, i.segments.first()) {
            (Some(colon), _) => colon.spans[0],
            (None, Some(first)) => first.ident.span(),
            (None, None) => Span::call_site(),
        };
        self.check(&names::written(i), span);
        visit::visit_path(self, i);
    }

    fn visit_expr_path(&mut self, i: &'ast syn::ExprPath) {
        match &i.qself {
            Some(qself) if qself.position == 0 => {
                for attr in &i.attrs {
                    self.visit_attribute(attr);
                }
                self.visit_qualified(qself, &i.path);
            }
            _ => visit::visit_expr_path(self, i),
        }
    }

    fn visit_type_path(&mut self, i: &'ast syn::TypePath) {
        match &i.qself {
            Some(qself) if qself.position == 0 => self.visit_qualified(qself, &i.path),
            _ => visit::visit_type_path(self, i),
        }
    }

    fn visit_macro(&mut self, i: &'ast syn::Macro) {
        self.check_tokens(i.tokens.clone());
        visit::visit_macro(self, i);
    }

    fn visit_meta_list(&mut self, i: &'ast syn::MetaList) {
        self.check_tokens(i.tokens.clone());
        visit::visit_meta_list(self, i);
    }
}

/// The `a::b::c` chains of a token stream and its groups, each with the span of its start: a
/// leading `::` counts, and `<a::b>::c` reads as `a::b::c` at the `<`.
fn chains(tokens: TokenStream) -> Vec<(Written, Span)> {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        match token {
            TokenTree::Group(group) => {
                found.extend(chains(group.stream()));
                index += 1;
            }
            TokenTree::Ident(ident) => {
                let mut path = Written {
                    segments: vec![ident.to_string()],
                    leading_colon: false,
                };
                let mut span = ident.span();
                let mut end = follow(&tokens, index + 1, &mut path.segments);
                let opened = index
                    .checked_sub(1)
                    .and_then(|before| tokens.get(before))
                    .is_some_and(|before| is_punct(before, '<'));
                if opened
                    && tokens.get(end).is_some_and(|after| is_punct(after, '>'))
                    && is_path_sep(&tokens, end + 1)
                {
                    if let Some(TokenTree::Punct(lt)) = tokens.get(index - 1) {
                        span = lt.span();
                    }
                    end = follow(&tokens, end + 1, &mut path.segments);
                }
                found.push((path, span));
                index = end;
            }
            TokenTree::Punct(colon) if is_path_sep(&tokens, index) => {
                if let Some(TokenTree::Ident(ident)) = tokens.get(index + 2) {
                    let mut path = Written {
                        segments: vec![ident.to_string()],
                        leading_colon: true,
                    };
                    let end = follow(&tokens, index + 3, &mut path.segments);
                    found.push((path, colon.span()));
                    index = end;
                } else {
                    index += 2;
                }
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => index += 1,
        }
    }
    found
}

/// Appends each `:: ident` from `index` on to `segments`; returns the index after the last.
fn follow(tokens: &[TokenTree], mut index: usize, segments: &mut Vec<String>) -> usize {
    while is_path_sep(tokens, index) {
        let Some(TokenTree::Ident(ident)) = tokens.get(index + 2) else {
            break;
        };
        segments.push(ident.to_string());
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
