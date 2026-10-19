//! Name resolution for the core-io check: what a path written in a held crate may point at.
//!
//! The first pass records, for every module of every crate root (the library, `main.rs`, each
//! binary), the names its `use` items, `type` aliases and `extern crate` items bind, the modules
//! it glob-imports, and every associated type (set in an impl, a trait or a bound such as
//! `T: K<C = X>`). `Table::resolve` then follows a written path through those bindings to every
//! place it may name: a path inside `std` or `core`, or a path inside the crate. `Self` inside
//! an `impl` stands for the impl's self type. Raw identifiers count without their `r#`. Code
//! inside a macro call or a `macro_rules!` rule that parses as items, statements or expressions
//! is read like the code around it; a metavariable there is a placeholder segment
//! (`tokens.rs`), which resolves like any name the crate does not bind.
//!
//! Scopes are approximated: a `use` in a function body binds its name for the whole module; a
//! name with several bindings may point at any of them; `std` and every
//! `extern crate std as x` name are reachable everywhere, even where a local item shadows them;
//! `<_ as _>::X` may be any associated type `X` of the crate, and so may `Self::X` and `T::X`
//! where nothing in scope sets `X` (the second pass reads what the enclosing impl or trait and
//! the bounds on `T` set, `scopes.rs`). The modules come from the `mod` items (see
//! `files.rs`); a file that no root reaches is placed by its path under `src/`.

use super::tokens;
use proc_macro2::Span;
use std::collections::{BTreeMap, BTreeSet};
use syn::ext::IdentExt;
use syn::visit::{self, Visit};

/// A module path. A library module has no prefix (`[]` is the library root); a module of a
/// binary starts with that binary's root, a segment no identifier can spell (`{bin x}`).
pub(super) type Module = Vec<String>;

/// A path as written: its segments (without `r#`) and whether it starts with `::`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Written {
    pub segments: Vec<String>,
    pub leading_colon: bool,
}

/// One place a path may point at.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Target {
    /// A path inside the standard library, starting with `std` (or `core`, written as such).
    Std(Vec<String>),
    /// A path inside the crate, its root first (see `Module`); also `$p::...` for a
    /// metavariable `$p` written as `::$p::...`.
    Crate(Vec<String>),
}

/// The bindings of every module of one crate.
#[derive(Debug, Default)]
pub(super) struct Table {
    /// The paths each name of each module is bound to.
    bindings: BTreeMap<(Module, String), Vec<Written>>,
    /// The prefixes each module glob-imports (`use a::b::*;` records `a::b`).
    globs: BTreeMap<Module, Vec<Written>>,
    /// The names of `extern crate std as <name>;`.
    std_names: BTreeSet<String>,
    /// The names of `extern crate self as <name>;`, each with the roots it names.
    crate_names: BTreeMap<String, BTreeSet<Module>>,
    /// Every associated type by name, with its module and the type it is bound to.
    associated: BTreeMap<String, Vec<(Module, Written)>>,
}

/// How deep `Table::resolve` follows bindings before it stops; a cycle stops sooner.
const DEPTH: usize = 64;

/// The bindings followed so far, to stop on a cycle.
type Stack = Vec<(Module, String)>;

impl Table {
    /// Records the bindings of one parsed file, whose module is `module`.
    pub(super) fn add_file(&mut self, module: &[String], file: &syn::File) {
        Collector {
            table: self,
            module: module.to_vec(),
            self_types: Vec::new(),
        }
        .visit_file(file);
    }

    /// Every place `path`, written in `module`, may point at.
    pub(super) fn resolve(&self, module: &[String], path: &Written) -> BTreeSet<Target> {
        self.resolve_in(module, path, &mut Vec::new())
    }

    /// Every place `<associated type called name>::<rest>` may point at, through each
    /// associated type of that name in the crate.
    pub(super) fn associated(&self, name: &str, rest: &[String]) -> BTreeSet<Target> {
        let mut found = BTreeSet::new();
        for (module, path) in self.associated.get(name).into_iter().flatten() {
            let mut path = path.clone();
            path.segments.extend(rest.iter().cloned());
            found.extend(self.resolve(module, &path));
        }
        found
    }

    fn resolve_in(&self, module: &[String], path: &Written, stack: &mut Stack) -> BTreeSet<Target> {
        let mut found = BTreeSet::new();
        let Some((first, rest)) = path.segments.split_first() else {
            return found;
        };
        if first == "std" || self.std_names.contains(first) {
            found.insert(Target::Std(std_path("std", rest)));
        }
        // `core` holds none of the banned paths, but `core::include!` is `include!`.
        if first == "core" {
            found.insert(Target::Std(std_path("core", rest)));
        }
        for root in self.crate_names.get(first).into_iter().flatten() {
            found.extend(self.walk(root, rest, stack));
        }
        if path.leading_colon {
            // `::$p::...` names whatever crate `$p` expands to, so it is built from `$p`.
            if tokens::is_placeholder(first) {
                found.insert(Target::Crate(path.segments.clone()));
            }
            return found;
        }
        match first.as_str() {
            "crate" => found.extend(self.walk(root_of(module), rest, stack)),
            "self" => found.extend(self.walk(module, rest, stack)),
            "super" => found.extend(self.walk(parent(module), rest, stack)),
            _ => {
                for target in self.name(module, first, stack) {
                    found.extend(self.extend(target, rest, stack));
                }
            }
        }
        found
    }

    /// The places `rest` names, read from inside the crate module `module`.
    fn walk(&self, module: &[String], rest: &[String], stack: &mut Stack) -> BTreeSet<Target> {
        let Some((first, tail)) = rest.split_first() else {
            return BTreeSet::from([Target::Crate(module.to_vec())]);
        };
        match first.as_str() {
            "self" => self.walk(module, tail, stack),
            "super" => self.walk(parent(module), tail, stack),
            _ => {
                let mut found = BTreeSet::new();
                for target in self.name(module, first, stack) {
                    found.extend(self.extend(target, tail, stack));
                }
                found
            }
        }
    }

    /// `target` followed by the segments `rest`.
    fn extend(&self, target: Target, rest: &[String], stack: &mut Stack) -> BTreeSet<Target> {
        match target {
            Target::Std(mut path) => {
                path.extend(rest.iter().cloned());
                BTreeSet::from([Target::Std(path)])
            }
            Target::Crate(path) => self.walk(&path, rest, stack),
        }
    }

    /// Every place the name `name` may point at in `module`: an item of that module of the same
    /// name, what a binding of the name points at, and the name in each glob-imported module.
    fn name(&self, module: &[String], name: &str, stack: &mut Stack) -> BTreeSet<Target> {
        let mut item = module.to_vec();
        item.push(name.to_owned());
        let mut found = BTreeSet::from([Target::Crate(item)]);
        let key = (module.to_vec(), name.to_owned());
        if stack.len() >= DEPTH || stack.contains(&key) {
            return found;
        }
        stack.push(key.clone());
        for path in self.bindings.get(&key).into_iter().flatten() {
            found.extend(self.resolve_in(module, path, stack));
        }
        for prefix in self.globs.get(module).into_iter().flatten() {
            for target in self.resolve_in(module, prefix, stack) {
                if let Target::Crate(glob) = target {
                    found.extend(self.name(&glob, name, stack));
                }
            }
        }
        stack.pop();
        found
    }
}

/// `root` (`std` or `core`) followed by `rest`.
fn std_path(root: &str, rest: &[String]) -> Vec<String> {
    let mut path = vec![root.to_owned()];
    path.extend(rest.iter().cloned());
    path
}

/// The crate root `module` belongs to: its binary root segment, or `[]` for the library.
fn root_of(module: &[String]) -> &[String] {
    match module.first() {
        Some(first) if first.starts_with('{') => module.get(..1).unwrap_or_default(),
        _ => &[],
    }
}

/// The parent of `module`; a crate root is its own parent.
fn parent(module: &[String]) -> &[String] {
    if module.len() > root_of(module).len() {
        module.split_last().map_or(module, |(_, parent)| parent)
    } else {
        module
    }
}

/// An identifier as a name: without the `r#` of a raw identifier.
pub(super) fn name_of(ident: &proc_macro2::Ident) -> String {
    ident.unraw().to_string()
}

/// The module segment of `mod <ident>`. A module named by a metavariable gets a segment no
/// identifier can spell, so the paths inside it do not read as built from that metavariable.
pub(super) fn module_name(ident: &proc_macro2::Ident) -> String {
    let name = name_of(ident);
    if tokens::is_placeholder(&name) {
        format!("<mod {}>", tokens::render(&name))
    } else {
        name
    }
}

/// The path of a type, looking through parentheses and invisible groups; `None` for any other
/// type and for a qualified path.
pub(super) fn type_path(ty: &syn::Type) -> Option<&syn::Path> {
    match ty {
        syn::Type::Paren(inner) => type_path(&inner.elem),
        syn::Type::Group(inner) => type_path(&inner.elem),
        syn::Type::Path(path) if path.qself.is_none() => Some(&path.path),
        _ => None,
    }
}

/// `path` with a leading `Self` replaced by `self_type`, the self type of the enclosing impl.
pub(super) fn with_self(path: Written, self_type: Option<&Written>) -> Written {
    match (path.segments.split_first(), self_type) {
        (Some((first, rest)), Some(self_type)) if first == "Self" && !path.leading_colon => {
            let mut segments = self_type.segments.clone();
            segments.extend(rest.iter().cloned());
            Written {
                segments,
                leading_colon: self_type.leading_colon,
            }
        }
        _ => path,
    }
}

/// The self type of an impl, as a path, when it is one.
pub(super) fn impl_self_type(item: &syn::ItemImpl) -> Option<Written> {
    type_path(&item.self_ty).map(written)
}

/// One leaf of a `use` tree.
#[derive(Debug, Clone)]
pub(super) enum Leaf {
    /// `a::b`, `a::b as c` or `a::{self}`: the path imported, the name it binds (`None` for
    /// `as _`) and the span of the imported name.
    Bind {
        path: Written,
        name: Option<String>,
        span: Span,
    },
    /// `a::b::*`: the prefix and the span of the `*`.
    Glob { prefix: Written, span: Span },
}

/// The leaves of a `use` item, each with its full path.
pub(super) fn use_leaves(item: &syn::ItemUse) -> Vec<Leaf> {
    let mut leaves = Vec::new();
    let leading_colon = item.leading_colon.is_some();
    collect_leaves(&item.tree, &mut Vec::new(), leading_colon, &mut leaves);
    leaves
}

fn collect_leaves(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    leading_colon: bool,
    leaves: &mut Vec<Leaf>,
) {
    let written = |segments: Vec<String>| Written {
        segments,
        leading_colon,
    };
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(name_of(&path.ident));
            collect_leaves(&path.tree, prefix, leading_colon, leaves);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            let (segments, bound) = imported(prefix, &name.ident);
            leaves.push(Leaf::Bind {
                path: written(segments),
                name: bound,
                span: name.ident.span(),
            });
        }
        syn::UseTree::Rename(rename) => {
            let (segments, _) = imported(prefix, &rename.ident);
            let bound = name_of(&rename.rename);
            leaves.push(Leaf::Bind {
                path: written(segments),
                name: (bound != "_").then_some(bound),
                span: rename.ident.span(),
            });
        }
        syn::UseTree::Glob(glob) => leaves.push(Leaf::Glob {
            prefix: written(prefix.clone()),
            span: glob.star_token.spans[0],
        }),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_leaves(item, prefix, leading_colon, leaves);
            }
        }
    }
}

/// The path `prefix::ident` imports and the name it binds: `a::{self}` imports `a` as `a`.
fn imported(prefix: &[String], ident: &syn::Ident) -> (Vec<String>, Option<String>) {
    let mut segments = prefix.to_vec();
    if ident == "self" {
        let bound = segments.last().cloned();
        (segments, bound)
    } else {
        segments.push(name_of(ident));
        (segments, Some(name_of(ident)))
    }
}

/// The segments of a `syn::Path`, as written, without `r#`.
pub(super) fn written(path: &syn::Path) -> Written {
    Written {
        segments: path
            .segments
            .iter()
            .map(|segment| name_of(&segment.ident))
            .collect(),
        leading_colon: path.leading_colon.is_some(),
    }
}

/// The first pass: records bindings into a `Table`.
struct Collector<'t> {
    table: &'t mut Table,
    module: Module,
    /// The self type of each enclosing impl (`None` for a trait or a self type that is no path).
    self_types: Vec<Option<Written>>,
}

impl Collector<'_> {
    fn bind(&mut self, name: String, path: Written) {
        let path = self.with_self(path);
        self.table
            .bindings
            .entry((self.module.clone(), name))
            .or_default()
            .push(path);
    }

    fn with_self(&self, path: Written) -> Written {
        with_self(path, self.self_types.last().and_then(Option::as_ref))
    }

    fn associated(&mut self, ident: &syn::Ident, ty: &syn::Type) {
        if let Some(path) = type_path(ty) {
            let path = self.with_self(written(path));
            self.table
                .associated
                .entry(name_of(ident))
                .or_default()
                .push((self.module.clone(), path));
        }
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        self.module.push(module_name(&i.ident));
        visit::visit_item_mod(self, i);
        self.module.pop();
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        self.self_types.push(impl_self_type(i));
        visit::visit_item_impl(self, i);
        self.self_types.pop();
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        self.self_types.push(None);
        visit::visit_item_trait(self, i);
        self.self_types.pop();
    }

    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        for leaf in use_leaves(i) {
            match leaf {
                Leaf::Bind {
                    path,
                    name: Some(name),
                    ..
                } => self.bind(name, path),
                Leaf::Bind { name: None, .. } => {}
                Leaf::Glob { prefix, .. } => self
                    .table
                    .globs
                    .entry(self.module.clone())
                    .or_default()
                    .push(prefix),
            }
        }
    }

    fn visit_item_type(&mut self, i: &'ast syn::ItemType) {
        if let Some(alias) = type_path(&i.ty) {
            self.bind(name_of(&i.ident), written(alias));
        }
        visit::visit_item_type(self, i);
    }

    fn visit_impl_item_type(&mut self, i: &'ast syn::ImplItemType) {
        self.associated(&i.ident, &i.ty);
        visit::visit_impl_item_type(self, i);
    }

    fn visit_assoc_type(&mut self, i: &'ast syn::AssocType) {
        self.associated(&i.ident, &i.ty);
        visit::visit_assoc_type(self, i);
    }

    fn visit_trait_item_type(&mut self, i: &'ast syn::TraitItemType) {
        if let Some((_, ty)) = &i.default {
            self.associated(&i.ident, ty);
        }
        visit::visit_trait_item_type(self, i);
    }

    fn visit_item_extern_crate(&mut self, i: &'ast syn::ItemExternCrate) {
        let Some((_, rename)) = &i.rename else {
            return;
        };
        let name = name_of(rename);
        if i.ident == "std" {
            self.table.std_names.insert(name);
        } else if i.ident == "self" {
            let root = root_of(&self.module).to_vec();
            self.table.crate_names.entry(name).or_default().insert(root);
        }
    }

    fn visit_macro(&mut self, i: &'ast syn::Macro) {
        for body in tokens::bodies(i) {
            body.visit(self);
        }
        visit::visit_macro(self, i);
    }
}
