//! What `runner-stderr` judges in a file's `use` items: a `use` that brings `io::stderr` in under
//! a bare name (so a bare `stderr` could name it), and, in the runner's files, a `use crate::` of a
//! name outside the helper modules the check reads.

use super::calls::Hit;
use super::{Fix, HELPER_MODULES};
use proc_macro2::Ident;
use std::collections::BTreeSet;

/// The message for a `use` that brings `io::stderr` in under a bare name.
const STDERR_IMPORT: &str = "this `use` brings io::stderr in under a bare name; the runner says \
                             things only through runner::stderr";
/// The roots of a path into the sandbox crate: `crate` in it, `clauseo_sandbox` in its binary.
const CRATE_ROOTS: [&str; 2] = ["crate", "clauseo_sandbox"];

/// The `use` items in `tree` that bring `io::stderr` in under a bare name: `io::stderr`,
/// `io::{stderr}`, `io::stderr as x`, `io::*`. `parent` is the segment before `tree`.
pub(super) fn stderr_imports(tree: &syn::UseTree, parent: Option<&Ident>, hits: &mut Vec<Hit>) {
    let from_io = parent.is_some_and(|parent| parent == "io");
    let at = match tree {
        syn::UseTree::Path(path) => return stderr_imports(&path.tree, Some(&path.ident), hits),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                stderr_imports(item, parent, hits);
            }
            return;
        }
        syn::UseTree::Name(name) if from_io && name.ident == "stderr" => name.ident.span(),
        syn::UseTree::Rename(rename) if from_io && rename.ident == "stderr" => rename.ident.span(),
        syn::UseTree::Glob(glob) if from_io => glob.star_token.span,
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => return,
    };
    hits.push(Hit {
        at,
        message: STDERR_IMPORT.to_owned(),
        fix: Fix::StderrValue,
    });
}

/// The `use crate::<name>` items in `tree` (or `clauseo_sandbox::<name>`) whose name is neither
/// a helper module nor one of the root's `exports` from one: what the runner imports from
/// outside the helpers the check reads.
pub(super) fn outside_imports(
    tree: &syn::UseTree,
    exports: &BTreeSet<String>,
    hits: &mut Vec<Hit>,
) {
    match tree {
        syn::UseTree::Path(path) if CRATE_ROOTS.iter().any(|root| path.ident == root) => {
            crate_module(&path.tree, &path.ident, exports, hits);
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                outside_imports(item, exports, hits);
            }
        }
        _ => {}
    }
}

/// Judges the name `tree` imports right after the crate's `root`.
fn crate_module(
    tree: &syn::UseTree,
    root: &Ident,
    exports: &BTreeSet<String>,
    hits: &mut Vec<Hit>,
) {
    let (name, at) = match tree {
        syn::UseTree::Path(path) => (path.ident.to_string(), path.ident.span()),
        syn::UseTree::Name(name) => (name.ident.to_string(), name.ident.span()),
        syn::UseTree::Rename(rename) => (rename.ident.to_string(), rename.ident.span()),
        syn::UseTree::Glob(glob) => ("*".to_owned(), glob.star_token.span),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                crate_module(item, root, exports, hits);
            }
            return;
        }
    };
    if !HELPER_MODULES.contains(&name.as_str()) && !exports.contains(&name) {
        hits.push(Hit {
            at,
            message: format!(
                "`use {root}::{name}` imports from outside the helpers runner-stderr reads \
                 (crate::{})",
                HELPER_MODULES.join(", crate::")
            ),
            fix: Fix::OutsideHelpers,
        });
    }
}

/// Adds each name `tree` brings in to `names`: a leaf's name, or its rename.
pub(super) fn leaves(tree: &syn::UseTree, names: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Path(path) => leaves(&path.tree, names),
        syn::UseTree::Name(name) => {
            names.insert(name.ident.to_string());
        }
        syn::UseTree::Rename(rename) => {
            names.insert(rename.rename.to_string());
        }
        syn::UseTree::Glob(_) => {}
        syn::UseTree::Group(group) => {
            for item in &group.items {
                leaves(item, names);
            }
        }
    }
}
