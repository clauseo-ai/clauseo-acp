//! The second pass of the core-io check: every place a parsed file reaches a banned path.
//!
//! It visits every `use` leaf, every path (in expressions, patterns, types, attributes and
//! macro names) and every qualified path `<T>::f`, and resolves each through the crate's
//! `Table`. Macro bodies that parse are visited like other code; the rest, and the tokens of
//! attribute lists, are walked for chains (`tokens.rs`). A chain that a metavariable continues
//! or starts fails when its expansion could reach a banned path. It also fails `#[path]` on a
//! module and `include!`, which pull in code the check never parses.

use super::bans::Bans;
use super::names::{self, Leaf, Module, Table, Target, Written};
use super::tokens::{self, Body, Chain, Kind};
use proc_macro2::{Span, TokenStream};
use std::collections::BTreeSet;
use syn::visit::{self, Visit};

/// What a site does wrong.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Reach {
    /// Reaches a banned path: `std::fs`, `std::time::Instant::now`.
    Uses(String),
    /// A glob import of `prefix` (`std::time`), which reaches these banned paths.
    Glob {
        prefix: String,
        reaches: Vec<String>,
    },
    /// A path built from a macro metavariable (`std::$x`, `$t::now`), which may reach these.
    Metavariable {
        written: String,
        reaches: Vec<String>,
    },
    /// `#[path]` on the module `module`, with the path when it is a string.
    PathAttribute {
        module: String,
        path: Option<String>,
    },
    /// A call of `include!`.
    Include,
}

/// One offending site in a file. Two sites of one kind on one line are one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Site {
    /// 1-based.
    pub line: u32,
    pub reach: Reach,
}

/// Every offending site in `file`, whose module is `module`, sorted by line.
pub(super) fn sites(table: &Table, bans: Bans, module: &[String], file: &syn::File) -> Vec<Site> {
    let mut finder = Finder {
        table,
        bans,
        module: module.to_vec(),
        self_types: Vec::new(),
        found: BTreeSet::new(),
    };
    finder.visit_file(file);
    finder.found.into_iter().collect()
}

/// The second pass: finds the sites of one file.
struct Finder<'t> {
    table: &'t Table,
    bans: Bans,
    module: Module,
    /// The self type of each enclosing impl (`None` for a trait or a self type that is no path).
    self_types: Vec<Option<Written>>,
    found: BTreeSet<Site>,
}

impl Finder<'_> {
    /// Every path inside `std` that `path` may point at, `Self` read as the impl's self type.
    fn std_targets(&self, path: &Written) -> Vec<Vec<String>> {
        let path = names::with_self(
            path.clone(),
            self.self_types.last().and_then(Option::as_ref),
        );
        self.table
            .resolve(&self.module, &path)
            .into_iter()
            .filter_map(|target| match target {
                Target::Std(found) => Some(found),
                Target::Crate(_) => None,
            })
            .collect()
    }

    /// The first banned path `path` may reach, if any: through its bindings, or as
    /// `<associated type that is a clock>::now`.
    fn reach(&self, path: &Written) -> Option<String> {
        let direct = self
            .std_targets(path)
            .into_iter()
            .find_map(|found| self.bans.banned(&found));
        direct.or_else(|| match path.segments.as_slice() {
            [.., associated, now] if now == "now" => self
                .table
                .associated_clock(associated)
                .map(|clock| format!("std::time::{clock}::now")),
            _ => None,
        })
    }

    /// Adds a site at `span` when `path` may reach a banned path.
    fn check(&mut self, path: &Written, span: Span) {
        if let Some(reach) = self.reach(path) {
            self.add(span, Reach::Uses(reach));
        }
    }

    fn add(&mut self, span: Span, reach: Reach) {
        let line = u32::try_from(span.start().line).unwrap_or(u32::MAX);
        self.found.insert(Site { line, reach });
    }

    /// Checks a qualified path `<T>::rest` as the path `T::rest`, at the `<`. When `T` itself is
    /// banned (`<std::fs::File>::open`), the site is `T`'s, found when `T` is visited.
    fn visit_qualified(&mut self, qself: &syn::QSelf, path: &syn::Path) {
        if let Some(ty) = names::type_path(&qself.ty)
            && self.reach(&names::written(ty)).is_none()
        {
            let mut joined = names::written(ty);
            joined.segments.extend(names::written(path).segments);
            self.check(&joined, qself.lt_token.spans[0]);
        }
        self.visit_qself(qself);
        for segment in &path.segments {
            self.visit_path_arguments(&segment.arguments);
        }
    }

    /// The banned paths a glob of, or a metavariable after, the std paths of `path` reach.
    fn reaches(&self, path: &Written) -> Option<(String, Vec<String>)> {
        self.std_targets(path).into_iter().find_map(|found| {
            let reaches = self.bans.reaches(&found);
            (!reaches.is_empty()).then(|| (found.join("::"), reaches))
        })
    }

    /// Checks the chains of tokens that did not parse (see the module docs).
    fn check_tokens(&mut self, tokens: TokenStream) {
        for Chain { path, span, kind } in tokens::chains(tokens) {
            match kind {
                Kind::Path => self.check(&path, span),
                Kind::ThenMetavariable(name) => {
                    if let Some(banned) = self.reach(&path) {
                        self.add(span, Reach::Uses(banned));
                    } else if let Some((prefix, reaches)) = self.reaches(&path) {
                        let written = format!("{prefix}::${name}");
                        self.add(span, Reach::Metavariable { written, reaches });
                    }
                }
                Kind::Metavariable(name) => {
                    let reaches = self.bans.metavariable_reaches(&path.segments);
                    if !reaches.is_empty() {
                        let written = format!("${name}::{}", path.segments.join("::"));
                        self.add(span, Reach::Metavariable { written, reaches });
                    }
                }
            }
        }
    }

    /// Fails `#[path]` (also inside `cfg_attr`) on a module.
    fn check_path_attribute(&mut self, item: &syn::ItemMod) {
        for attr in &item.attrs {
            let metas: Vec<syn::Meta> = if attr.path().is_ident("cfg_attr") {
                attr.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                )
                .map(|metas| metas.into_iter().skip(1).collect())
                .unwrap_or_default()
            } else {
                vec![attr.meta.clone()]
            };
            for meta in metas.iter().filter(|meta| meta.path().is_ident("path")) {
                let path = match meta {
                    syn::Meta::NameValue(syn::MetaNameValue {
                        value:
                            syn::Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Str(path),
                                ..
                            }),
                        ..
                    }) => Some(path.value()),
                    _ => None,
                };
                let module = names::name_of(&item.ident);
                let span = attr.pound_token.spans[0];
                self.add(span, Reach::PathAttribute { module, path });
            }
        }
    }
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        self.check_path_attribute(i);
        self.module.push(names::name_of(&i.ident));
        visit::visit_item_mod(self, i);
        self.module.pop();
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        self.self_types.push(names::impl_self_type(i));
        visit::visit_item_impl(self, i);
        self.self_types.pop();
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        self.self_types.push(None);
        visit::visit_item_trait(self, i);
        self.self_types.pop();
    }

    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        for attr in &i.attrs {
            self.visit_attribute(attr);
        }
        for leaf in names::use_leaves(i) {
            match leaf {
                Leaf::Bind { path, span, .. } => self.check(&path, span),
                Leaf::Glob { prefix, span } => {
                    if let Some((prefix, reaches)) = self.reaches(&prefix) {
                        self.add(span, Reach::Glob { prefix, reaches });
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
        let name = names::written(&i.path);
        let include = match name.segments.as_slice() {
            [include] => include == "include",
            [krate, include] => include == "include" && (krate == "std" || krate == "core"),
            _ => false,
        };
        if include {
            self.add(i.bang_token.spans[0], Reach::Include);
        }
        for body in tokens::bodies(i) {
            match body {
                Body::Items(items) => items.iter().for_each(|item| Visit::visit_item(self, item)),
                Body::Stmts(stmts) => stmts.iter().for_each(|stmt| Visit::visit_stmt(self, stmt)),
                Body::Exprs(exprs) => exprs.iter().for_each(|expr| Visit::visit_expr(self, expr)),
                Body::Tokens(tokens) => self.check_tokens(tokens),
            }
        }
        visit::visit_macro(self, i);
    }

    fn visit_meta_list(&mut self, i: &'ast syn::MetaList) {
        self.check_tokens(i.tokens.clone());
        visit::visit_meta_list(self, i);
    }
}
