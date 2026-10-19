//! The sources of a held crate: every `.rs` file under its `src/`, the symlinks there (which the
//! check does not follow), the target paths its `Cargo.toml` sets (failing those outside `src/`),
//! and the module each file is, found by following `mod` items from each crate root.

use super::names::{self, Module};
use crate::checks::{self, violation};
use crate::violation::Violation;
use crate::workspace::Member;
use proc_macro2::Span;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Component, Path};
use syn::punctuated::Punctuated;

/// What is under a crate's `src/`, repo-relative and sorted.
#[derive(Debug, Default)]
pub(super) struct Listing {
    pub files: Vec<String>,
    /// Symlinks to a directory or to a `.rs` file (or broken ones named `.rs`).
    pub symlinks: Vec<String>,
}

/// Lists the repo-relative directory `src` under `root`. A missing directory lists nothing.
pub(super) fn list(root: &Path, src: &str) -> io::Result<Listing> {
    let mut listing = Listing::default();
    let mut pending = vec![root.join(src)];
    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            let is_rust = path.extension().is_some_and(|ext| ext == "rs");
            if kind.is_symlink() {
                if is_rust || fs::metadata(&path).is_ok_and(|target| target.is_dir()) {
                    listing.symlinks.push(checks::repo_path(root, &path));
                }
            } else if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && is_rust {
                listing.files.push(checks::repo_path(root, &path));
            }
        }
    }
    listing.files.sort();
    listing.symlinks.sort();
    Ok(listing)
}

/// A `path` set for a target in `Cargo.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TargetPath {
    /// `[lib]` or `[[bin]]`.
    pub table: &'static str,
    pub path: String,
    /// The line of the path in `Cargo.toml`.
    pub line: u32,
    /// The path relative to the crate's `src/` (`lib.rs` for `src/lib.rs`), `None` when it is
    /// outside `src/`.
    pub in_src: Option<String>,
}

#[derive(Deserialize)]
struct Manifest {
    lib: Option<Target>,
    #[serde(default)]
    bin: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    path: Option<toml::Spanned<String>>,
}

/// The target paths of the manifest `text`; `Err` when it is not TOML of the expected shape.
pub(super) fn target_paths(text: &str) -> Result<Vec<TargetPath>, toml::de::Error> {
    let manifest: Manifest = toml::from_str(text)?;
    let lib = manifest.lib.into_iter().map(|target| ("[lib]", target));
    let bins = manifest.bin.into_iter().map(|target| ("[[bin]]", target));
    Ok(lib
        .chain(bins)
        .filter_map(|(table, target)| {
            let path = target.path?;
            let head = text.get(..path.span().start).unwrap_or(text);
            Some(TargetPath {
                table,
                line: u32::try_from(head.matches('\n').count() + 1).unwrap_or(u32::MAX),
                in_src: in_src(path.get_ref()),
                path: path.into_inner(),
            })
        })
        .collect())
}

/// The `[lib]` and `[[bin]]` paths of the crate's `Cargo.toml`; a path outside `src` and a
/// manifest that cannot be read go to `violations`.
pub(super) fn targets(
    root: &Path,
    member: &Member,
    src: &str,
    violations: &mut Vec<Violation>,
) -> Vec<TargetPath> {
    let manifest = checks::join(&member.dir, "Cargo.toml");
    let text = match checks::read_repo_file(root, &manifest) {
        Ok(Some(text)) => text,
        Ok(None) => return Vec::new(),
        Err(unreadable) => {
            violations.push(unreadable);
            return Vec::new();
        }
    };
    let targets = match target_paths(&text) {
        Ok(targets) => targets,
        Err(error) => {
            let start = error.span().map_or(0, |span| span.start);
            let head = text.get(..start).unwrap_or(&text);
            let line = u32::try_from(head.matches('\n').count() + 1).unwrap_or(u32::MAX);
            violations.push(violation(
                &manifest,
                line,
                format!(
                    "core-io cannot read the target paths of `{manifest}`: {}",
                    error.message()
                ),
                "fix the manifest so cargo and core-io can read it",
            ));
            return Vec::new();
        }
    };
    for target in targets.iter().filter(|target| target.in_src.is_none()) {
        violations.push(violation(
            &manifest,
            target.line,
            format!(
                "{}'s {} path `{}` is outside `{src}/`, and core-io parses only that folder",
                member.name, target.table, target.path
            ),
            format!("move the target's root file under `{src}/` and set `path` to it"),
        ));
    }
    targets
}

/// `path` (relative to the crate) relative to `src/`, when it lies there without `..`.
fn in_src(path: &str) -> Option<String> {
    let mut parts = Path::new(path)
        .components()
        .filter(|part| !matches!(part, Component::CurDir));
    if parts.next() != Some(Component::Normal("src".as_ref())) {
        return None;
    }
    let rest: Option<Vec<String>> = parts
        .map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    rest.filter(|rest| !rest.is_empty())
        .map(|rest| rest.join("/"))
}

/// One file and the module it is, `Module`'s root first. A file can be in several crate roots.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Placement {
    pub path: String,
    pub module: Module,
}

/// The crate roots under `src` (repo-relative) among `files`: the library (`lib.rs`, or the
/// `[lib]` path), `main.rs`, each `bin/<x>.rs` and `bin/<x>/main.rs`, and each `[[bin]]` path.
pub(super) fn roots(src: &str, files: &[String], targets: &[TargetPath]) -> Vec<(String, Module)> {
    let mut found = Vec::new();
    let lib = targets
        .iter()
        .find(|target| target.table == "[lib]")
        .and_then(|target| target.in_src.clone())
        .unwrap_or_else(|| "lib.rs".to_owned());
    found.push((lib, Vec::new()));
    found.push(("main.rs".to_owned(), vec!["{main}".to_owned()]));
    for file in files {
        let relative = file.strip_prefix(&format!("{src}/")).unwrap_or(file);
        let bin = match relative.split('/').collect::<Vec<_>>().as_slice() {
            ["bin", name] => name.strip_suffix(".rs"),
            ["bin", name, "main.rs"] => Some(*name),
            _ => None,
        };
        if let Some(name) = bin {
            found.push((relative.to_owned(), vec![format!("{{bin {name}}}")]));
        }
    }
    for target in targets.iter().filter(|target| target.table == "[[bin]]") {
        if let Some(relative) = &target.in_src {
            found.push((relative.clone(), vec![format!("{{bin {relative}}}")]));
        }
    }
    found
        .into_iter()
        .map(|(relative, module)| (checks::join(src, &relative), module))
        .filter(|(path, _)| files.contains(path))
        .collect()
}

/// Every file's placements: each root, then each file a root reaches through `mod` items (not
/// those with `#[path]`, also inside `cfg_attr`, which the check fails on its own). A file no
/// root reaches is placed by its path under `src` in the library.
pub(super) fn place(
    src: &str,
    roots: &[(String, Module)],
    parsed: &BTreeMap<String, syn::File>,
) -> Vec<Placement> {
    let mut placed = BTreeSet::new();
    for (path, module) in roots {
        let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        reach(path, dir, module, parsed, &mut placed);
    }
    let reached: BTreeSet<&String> = placed.iter().map(|placement| &placement.path).collect();
    let orphans: Vec<Placement> = parsed
        .keys()
        .filter(|path| !reached.contains(path))
        .map(|path| Placement {
            path: path.clone(),
            module: module_of(path.strip_prefix(&format!("{src}/")).unwrap_or(path)),
        })
        .collect();
    placed.extend(orphans);
    placed.into_iter().collect()
}

/// Places `path` as `module` and follows its `mod` items, whose files sit in `dir`.
fn reach(
    path: &str,
    dir: &str,
    module: &[String],
    parsed: &BTreeMap<String, syn::File>,
    placed: &mut BTreeSet<Placement>,
) {
    let placement = Placement {
        path: path.to_owned(),
        module: module.to_vec(),
    };
    if !placed.insert(placement) {
        return;
    }
    if let Some(file) = parsed.get(path) {
        follow(&file.items, dir, module, parsed, placed);
    }
}

fn follow(
    items: &[syn::Item],
    dir: &str,
    module: &[String],
    parsed: &BTreeMap<String, syn::File>,
    placed: &mut BTreeSet<Placement>,
) {
    for item in items {
        let syn::Item::Mod(item) = item else {
            continue;
        };
        if !path_attributes(item).is_empty() {
            continue;
        }
        let name = names::name_of(&item.ident);
        let mut child = module.to_vec();
        child.push(name.clone());
        let child_dir = checks::join(dir, &name);
        if let Some((_, inner)) = &item.content {
            follow(inner, &child_dir, &child, parsed, placed);
            continue;
        }
        let flat = format!("{child_dir}.rs");
        let nested = format!("{child_dir}/mod.rs");
        if parsed.contains_key(&flat) {
            reach(&flat, &child_dir, &child, parsed, placed);
        } else if parsed.contains_key(&nested) {
            reach(&nested, &child_dir, &child, parsed, placed);
        }
    }
}

/// Each `#[path]` on `item`, also one inside `#[cfg_attr(<predicate>, ...)]` at any depth: the
/// span of its `#` and the path when it is a string.
pub(super) fn path_attributes(item: &syn::ItemMod) -> Vec<(Span, Option<String>)> {
    let mut found = Vec::new();
    for attr in &item.attrs {
        let mut metas = Vec::new();
        path_metas(&attr.meta, &mut metas);
        for meta in metas {
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
            found.push((attr.pound_token.spans[0], path));
        }
    }
    found
}

/// Each `path` meta in `meta`, reading through nested `cfg_attr(<predicate>, ...)`.
fn path_metas(meta: &syn::Meta, found: &mut Vec<syn::Meta>) {
    if meta.path().is_ident("path") {
        found.push(meta.clone());
    } else if let syn::Meta::List(list) = meta
        && list.path.is_ident("cfg_attr")
    {
        let parser = Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated;
        for inner in list.parse_args_with(parser).into_iter().flatten().skip(1) {
            path_metas(&inner, found);
        }
    }
}

/// The library module of a file at `relative` under `src/`: `lib.rs` and `main.rs` are the root,
/// `a/mod.rs` and `a.rs` are `a`.
pub(super) fn module_of(relative: &str) -> Module {
    let mut parts: Vec<String> = relative
        .trim_end_matches(".rs")
        .split('/')
        .map(str::to_owned)
        .collect();
    let root_file = parts.len() == 1
        && parts
            .first()
            .is_some_and(|only| only == "lib" || only == "main");
    if root_file || parts.last().is_some_and(|last| last == "mod") {
        parts.pop();
    }
    parts
}
