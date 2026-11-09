//! The filters a rule is made of, lists of them from named paths, and the paths a rule's
//! filters name.

use std::path::PathBuf;

use super::Writer;

pub(super) fn subpath(param: &str) -> String {
    format!("(subpath {param})")
}

pub(super) fn literal(param: &str) -> String {
    format!("(literal {param})")
}

/// The folders above the path (not the path itself).
pub(super) fn ancestors(param: &str) -> String {
    format!("(path-ancestors {param})")
}

/// The folders above the path, as folders: renaming one would move the path out of its rule.
/// Above the matter they are write-denied anyway.
pub(super) fn directory_ancestors(param: &str) -> String {
    format!("(require-all (vnode-type DIRECTORY) (path-ancestors {param}))")
}

/// The paths of `params` that `filters` name through `(literal (param "X"))` or
/// `(subpath (param "X"))`, wherever in a filter they sit, but parameter `skip`'s.
pub(super) fn named_paths(
    filters: &[String],
    params: &[(String, PathBuf)],
    skip: &str,
) -> Vec<PathBuf> {
    let mut named = Vec::new();
    for filter in filters {
        for form in ["(literal (param \"", "(subpath (param \""] {
            for (at, _) in filter.match_indices(form) {
                let rest = filter.get(at + form.len()..).unwrap_or_default();
                let Some(name) = rest.split('"').next().filter(|name| *name != skip) else {
                    continue;
                };
                if let Some((_, path)) = params.iter().find(|(n, _)| n == name) {
                    named.push(path.clone());
                }
            }
        }
    }
    named
}

/// `subpath` filters for `list`, each path made a parameter of `w`.
pub(super) fn subpaths(w: &mut Writer, list: &[(String, PathBuf)]) -> Vec<String> {
    list.iter()
        .map(|(name, path)| subpath(&w.param(name, path)))
        .collect()
}

/// `literal` filters for `list`, each path made a parameter of `w`.
pub(super) fn literals(w: &mut Writer, list: &[(String, PathBuf)]) -> Vec<String> {
    list.iter()
        .map(|(name, path)| literal(&w.param(name, path)))
        .collect()
}

/// The folders above each path of `list`, each path made a parameter of `w`.
pub(super) fn ancestors_of(w: &mut Writer, list: &[(String, PathBuf)]) -> Vec<String> {
    list.iter()
        .map(|(name, path)| ancestors(&w.param(name, path)))
        .collect()
}
