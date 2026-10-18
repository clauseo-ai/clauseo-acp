//! Name resolution for the core-io check: what a path written in a held crate may point at.
//!
//! The first pass records, for every module of the crate, the names its `use` items, `type`
//! aliases and `extern crate` items bind, and the modules it glob-imports. `Table::resolve` then
//! follows a written path through those bindings to every place it may name: a path inside `std`,
//! or a path inside this crate.
//!
//! Scopes are over-approximated, which can only make the check stricter: a `use` in a function
//! body binds its name for the whole module; a name with several bindings (a `use` in one
//! function and a `type` in another) may point at any of them; `std` and every
//! `extern crate std as x` name are reachable from every module, even where a local item shadows
//! them. A file's module path comes from its place under `src/` (`a/b.rs` and `a/b/mod.rs` are
//! `crate::a::b`); `#[path]` attributes are not followed, and `src/bin/x.rs` counts as
//! `crate::bin::x`.

use proc_macro2::Span;
use std::collections::{BTreeMap, BTreeSet};
use syn::visit::{self, Visit};

/// A module path inside the crate, without the leading `crate`: `[]` is the crate root.
pub(super) type Module = Vec<String>;

/// A path as written: its segments and whether it starts with `::`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Written {
    pub segments: Vec<String>,
    pub leading_colon: bool,
}

/// One place a path may point at.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Target {
    /// A path inside the standard library, starting with `std`.
    Std(Vec<String>),
    /// A path inside this crate, without the leading `crate`.
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
}

/// How deep `Table::resolve` follows bindings before it stops; a cycle stops sooner.
const DEPTH: usize = 64;

impl Table {
    /// Records the bindings of one parsed file, whose own module is `module`.
    pub(super) fn add_file(&mut self, module: &[String], file: &syn::File) {
        Collector {
            table: self,
            module: module.to_vec(),
        }
        .visit_file(file);
    }

    /// Every place `path`, written in `module`, may point at.
    pub(super) fn resolve(&self, module: &[String], path: &Written) -> BTreeSet<Target> {
        self.resolve_in(module, path, &mut Vec::new())
    }

    fn resolve_in(
        &self,
        module: &[String],
        path: &Written,
        stack: &mut Vec<(Module, String)>,
    ) -> BTreeSet<Target> {
        let mut found = BTreeSet::new();
        let Some((first, rest)) = path.segments.split_first() else {
            return found;
        };
        if first == "std" || self.std_names.contains(first) {
            found.insert(Target::Std(std_path(rest)));
        }
        if path.leading_colon {
            return found;
        }
        match first.as_str() {
            "crate" => found.extend(self.walk(&[], rest, stack)),
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
    fn walk(
        &self,
        module: &[String],
        rest: &[String],
        stack: &mut Vec<(Module, String)>,
    ) -> BTreeSet<Target> {
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
    fn extend(
        &self,
        target: Target,
        rest: &[String],
        stack: &mut Vec<(Module, String)>,
    ) -> BTreeSet<Target> {
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
    fn name(
        &self,
        module: &[String],
        name: &str,
        stack: &mut Vec<(Module, String)>,
    ) -> BTreeSet<Target> {
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

/// `std` followed by `rest`.
fn std_path(rest: &[String]) -> Vec<String> {
    let mut path = vec!["std".to_owned()];
    path.extend(rest.iter().cloned());
    path
}

/// The parent of `module`; the crate root is its own parent.
fn parent(module: &[String]) -> &[String] {
    module.split_last().map_or(module, |(_, parent)| parent)
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
            prefix.push(path.ident.to_string());
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
            let bound = rename.rename.to_string();
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
        segments.push(ident.to_string());
        (segments, Some(ident.to_string()))
    }
}

/// The segments of a `syn::Path`, as written.
pub(super) fn written(path: &syn::Path) -> Written {
    Written {
        segments: path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect(),
        leading_colon: path.leading_colon.is_some(),
    }
}

/// The first pass: records bindings into a `Table`.
struct Collector<'t> {
    table: &'t mut Table,
    module: Module,
}

impl Collector<'_> {
    fn bind(&mut self, name: String, path: Written) {
        self.table
            .bindings
            .entry((self.module.clone(), name))
            .or_default()
            .push(path);
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        self.module.push(i.ident.to_string());
        visit::visit_item_mod(self, i);
        self.module.pop();
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
        if let syn::Type::Path(alias) = &*i.ty
            && alias.qself.is_none()
        {
            self.bind(i.ident.to_string(), written(&alias.path));
        }
        visit::visit_item_type(self, i);
    }

    fn visit_item_extern_crate(&mut self, i: &'ast syn::ItemExternCrate) {
        if i.ident == "std"
            && let Some((_, rename)) = &i.rename
        {
            self.table.std_names.insert(rename.to_string());
        }
    }
}
