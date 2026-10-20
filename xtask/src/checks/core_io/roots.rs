//! A held crate's target roots and build script. Every target root sits under the crate's folder
//! and carries `FORBID_LINE` among its inner attributes, and the crate has no build script.

use super::clippy_toml::line_number;
use super::report;
use crate::checks;
use crate::violation::Violation;
use crate::workspace::{self, Member};
use std::collections::BTreeSet;
use std::path::Path;
use syn::punctuated::Punctuated;
use syn::{AttrStyle, Attribute, Meta, Token};
use toml::de::DeTable;

/// The lints the forbid attribute names, in the order it names them.
const FORBIDDEN: [&str; 2] = ["clippy::disallowed_methods", "clippy::disallowed_types"];

/// The kind of a build script target, which `build_script` covers.
const BUILD_SCRIPT_KIND: &str = "custom-build";

/// The violations for the target roots of the held crate `member` that lie outside its folder,
/// lack the forbid line or do not parse, each root once, in the order `cargo metadata` lists
/// them.
pub(super) fn forbid_lines(root: &Path, member: &Member) -> Vec<Violation> {
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    let roots = member
        .targets
        .iter()
        .filter(|target| !target.kinds.iter().any(|kind| kind == BUILD_SCRIPT_KIND));
    for target in roots {
        if !seen.insert(target.src_path.as_str()) {
            continue;
        }
        let path = &target.src_path;
        if target.outside || !workspace::is_under(path, &member.dir) {
            let manifest = checks::join(&member.dir, "Cargo.toml");
            let kinds = target.kinds.join(", ");
            found.push(report::root_outside(
                &manifest,
                &member.name,
                &kinds,
                path,
                &member.dir,
            ));
            continue;
        }
        let text = match checks::read_repo_file(root, path) {
            Ok(Some(text)) => text,
            // cargo metadata listed it; a root gone since then fails cargo.
            Ok(None) => continue,
            Err(unreadable) => {
                found.push(unreadable);
                continue;
            }
        };
        match syn::parse_file(&text) {
            Ok(file) if file.attrs.iter().any(is_forbid_line) => {}
            Ok(_) => {
                let kinds = target.kinds.join(", ");
                found.push(report::missing_forbid(path, &member.name, &kinds));
            }
            Err(error) => {
                let line = u32::try_from(error.span().start().line).unwrap_or(1).max(1);
                found.push(report::root_not_rust(path, &member.name, line, &error));
            }
        }
    }
    found
}

/// True when `attr` is the inner attribute `#![forbid(..)]` naming exactly `FORBIDDEN`, in order.
fn is_forbid_line(attr: &Attribute) -> bool {
    if !matches!(attr.style, AttrStyle::Inner(_)) {
        return false;
    }
    let Meta::List(list) = &attr.meta else {
        return false;
    };
    if !list.path.is_ident("forbid") {
        return false;
    }
    let Ok(paths) = list.parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
    else {
        return false;
    };
    let names: Vec<String> = paths.iter().map(path_text).collect();
    names == FORBIDDEN
}

/// `path` as `a::b`, or empty for a path with a leading `::` or generic arguments.
fn path_text(path: &syn::Path) -> String {
    let plain = path.leading_colon.is_none()
        && path
            .segments
            .iter()
            .all(|segment| segment.arguments.is_none());
    if !plain {
        return String::new();
    }
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    segments.join("::")
}

/// The violations for the held crate `member`'s build script: a `build.rs` in its folder, and a
/// `build` key under `[package]` in its manifest, whatever its value.
pub(super) fn build_script(root: &Path, member: &Member) -> Vec<Violation> {
    let mut found = Vec::new();
    let script = checks::join(&member.dir, "build.rs");
    match checks::read_repo_file(root, &script) {
        Ok(Some(_)) => found.push(report::build_script_file(&script, &member.name)),
        Ok(None) => {}
        Err(unreadable) => found.push(unreadable),
    }
    let manifest = checks::join(&member.dir, "Cargo.toml");
    // A manifest that is missing or not TOML is `lints::crate_manifest`'s to report.
    let Ok(Some(text)) = checks::read_repo_file(root, &manifest) else {
        return found;
    };
    let Ok(doc) = DeTable::parse(&text) else {
        return found;
    };
    let build = doc
        .get_ref()
        .iter()
        .find(|(name, _)| name.get_ref() == "package")
        .and_then(|(_, package)| package.get_ref().as_table())
        .and_then(|package| package.iter().find(|(name, _)| name.get_ref() == "build"));
    if let Some((key, _)) = build {
        let line = line_number(&text, key.span().start);
        found.push(report::build_key(&manifest, line, &member.name));
    }
    found
}
