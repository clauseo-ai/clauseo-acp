//! The second pass of the core-io check: every place a parsed file reaches a banned path.
//!
//! It visits every `use` leaf, every path (in expressions, patterns, types and attributes) and
//! every qualified path `<T>::f`, and resolves each through the crate's `Table`. A macro's own
//! path is read only to find `include!`. Macro bodies that parse are visited like other code;
//! the rest, and the tokens of attribute lists, are walked for chains (`tokens.rs`). A path that
//! resolves through a metavariable's placeholder fails when what the metavariable could expand to
//! reaches a banned path (`bans.rs`). It also fails `#[path]` on a module and `include!`,
//! which pull in code the check never parses.

use super::bans::Bans;
use super::files;
use super::names::{self, Leaf, Module, Table, Target, Written};
use super::tokens::{self, Body, Chain};
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
    /// A path that resolves through a macro metavariable (`std::$x`, `$t::now`), written up to
    /// the metavariable or from it on, which may reach these.
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
        params: Vec::new(),
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
    /// The generic type parameters in scope.
    params: Vec<String>,
    found: BTreeSet<Site>,
}

impl Finder<'_> {
    /// Every path inside `std` that `path` may point at, `Self` read as the impl's self type.
    fn std_targets(&self, path: &Written) -> Vec<Vec<String>> {
        self.targets(path)
            .into_iter()
            .filter_map(|target| match target {
                Target::Std(found) => Some(found),
                Target::Crate(_) => None,
            })
            .collect()
    }

    /// Every place `path` may point at: through its bindings, `Self` read as the impl's self
    /// type, and through each associated type of its name when it is `Self::C::...` or
    /// `T::C::...` for a type parameter `T`.
    fn targets(&self, path: &Written) -> BTreeSet<Target> {
        let self_type = self.self_types.last().and_then(Option::as_ref);
        let mut found = self
            .table
            .resolve(&self.module, &names::with_self(path.clone(), self_type));
        if let [first, name, rest @ ..] = path.segments.as_slice()
            && !path.leading_colon
            && (first == "Self" || self.params.contains(first))
        {
            found.extend(self.table.associated(name, rest));
        }
        found
    }

    /// What reaching `targets` does wrong, if anything: a banned path inside `std`, else a
    /// path through a metavariable that may expand to a banned path.
    fn judge(&self, targets: BTreeSet<Target>) -> Option<Reach> {
        let mut built = None;
        for target in targets {
            let (written, reaches) = match target {
                Target::Std(path) => {
                    if let Some(banned) = self.bans.banned(&path) {
                        return Some(Reach::Uses(banned));
                    }
                    let Some(at) = path.iter().position(|seg| tokens::is_placeholder(seg)) else {
                        continue;
                    };
                    let reaches = self.bans.std_metavariable(&path).unwrap_or_default();
                    (tokens::join(path.get(..=at).unwrap_or_default()), reaches)
                }
                Target::Crate(path) => {
                    let Some(at) = path.iter().position(|seg| tokens::is_placeholder(seg)) else {
                        continue;
                    };
                    let from = path.get(at..).unwrap_or_default();
                    let rest = from.get(1..).unwrap_or_default();
                    (tokens::join(from), self.bans.metavariable_reaches(rest))
                }
            };
            if built.is_none() && !reaches.is_empty() {
                built = Some(Reach::Metavariable { written, reaches });
            }
        }
        built
    }

    /// Adds a site at `span` when `path` may reach a banned path.
    fn check(&mut self, path: &Written, span: Span) {
        if let Some(reach) = self.judge(self.targets(path)) {
            self.add(span, reach);
        }
    }

    fn add(&mut self, span: Span, reach: Reach) {
        let line = u32::try_from(span.start().line).unwrap_or(u32::MAX);
        self.found.insert(Site { line, reach });
    }

    /// Checks a qualified path `<T>::rest` as the path `T::rest`, at the `<`. When `T` itself
    /// reaches a banned path (`<std::fs::File>::open`), the site is `T`'s, found when `T` is
    /// visited.
    fn visit_qualified(&mut self, qself: &syn::QSelf, path: &syn::Path) {
        if let Some(ty) = names::type_path(&qself.ty)
            && self.judge(self.targets(&names::written(ty))).is_none()
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

    /// Checks `<T as K>::C::rest` through each associated type `C` of the crate, at the `<`.
    fn check_as_trait(&mut self, qself: &syn::QSelf, path: &syn::Path) {
        let tail = names::written(path).segments.split_off(qself.position);
        if let [name, rest @ ..] = tail.as_slice()
            && let Some(reach) = self.judge(self.table.associated(name, rest))
        {
            self.add(qself.lt_token.spans[0], reach);
        }
    }

    /// The banned paths a glob of the std paths of `path` reaches.
    fn reaches(&self, path: &Written) -> Option<(String, Vec<String>)> {
        self.std_targets(path).into_iter().find_map(|found| {
            let reaches = self.bans.reaches(&found);
            (!reaches.is_empty()).then(|| (tokens::join(&found), reaches))
        })
    }

    /// Checks the chains of tokens that did not parse.
    fn check_tokens(&mut self, tokens: TokenStream) {
        for Chain { path, span } in tokens::chains(tokens) {
            self.check(&path, span);
        }
    }

    /// Runs `visit` with the type parameters of `generics` in scope.
    fn with_params(&mut self, generics: Option<&syn::Generics>, visit: impl FnOnce(&mut Self)) {
        let depth = self.params.len();
        let params = generics.into_iter().flat_map(syn::Generics::type_params);
        self.params
            .extend(params.map(|param| names::name_of(&param.ident)));
        visit(self);
        self.params.truncate(depth);
    }
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_item(&mut self, i: &'ast syn::Item) {
        let generics = match i {
            syn::Item::Enum(item) => Some(&item.generics),
            syn::Item::Fn(item) => Some(&item.sig.generics),
            syn::Item::Impl(item) => Some(&item.generics),
            syn::Item::Struct(item) => Some(&item.generics),
            syn::Item::Trait(item) => Some(&item.generics),
            syn::Item::TraitAlias(item) => Some(&item.generics),
            syn::Item::Type(item) => Some(&item.generics),
            syn::Item::Union(item) => Some(&item.generics),
            _ => None,
        };
        self.with_params(generics, |me| visit::visit_item(me, i));
    }

    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        self.with_params(Some(&i.sig.generics), |me| visit::visit_impl_item_fn(me, i));
    }

    fn visit_trait_item_fn(&mut self, i: &'ast syn::TraitItemFn) {
        self.with_params(Some(&i.sig.generics), |me| {
            visit::visit_trait_item_fn(me, i);
        });
    }

    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        for (span, path) in files::path_attributes(i) {
            let module = names::name_of(&i.ident);
            self.add(span, Reach::PathAttribute { module, path });
        }
        self.module.push(names::module_name(&i.ident));
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
                    } else if let Some(built) = self.judge(self.targets(&prefix)) {
                        self.add(span, built);
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
            Some(qself) => {
                self.check_as_trait(qself, &i.path);
                visit::visit_expr_path(self, i);
            }
            None => visit::visit_expr_path(self, i),
        }
    }

    fn visit_type_path(&mut self, i: &'ast syn::TypePath) {
        match &i.qself {
            Some(qself) if qself.position == 0 => self.visit_qualified(qself, &i.path),
            Some(qself) => {
                self.check_as_trait(qself, &i.path);
                visit::visit_type_path(self, i);
            }
            None => visit::visit_type_path(self, i),
        }
    }

    /// Reads the macro's own path only for `include!`: `std::env!` is the compile-time macro,
    /// not the module.
    fn visit_macro(&mut self, i: &'ast syn::Macro) {
        let name = names::written(&i.path);
        let prelude = name.segments == ["include"] && !name.leading_colon;
        let include = || self.std_targets(&name).iter().any(|path| is_include(path));
        if prelude || include() {
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
    }

    fn visit_meta_list(&mut self, i: &'ast syn::MetaList) {
        self.check_tokens(i.tokens.clone());
        visit::visit_meta_list(self, i);
    }
}

/// True for `std::include` and `core::include`.
fn is_include(path: &[String]) -> bool {
    matches!(path, [root, name] if name == "include" && (root == "std" || root == "core"))
}
