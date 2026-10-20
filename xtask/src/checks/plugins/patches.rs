//! Patched Tauri crates. The graph rule trusts what `tauri` and the fixed plugins pull in, so
//! a `[patch]` or `[replace]` entry that swaps one of them for another source (a path, a git
//! fork, another registry) could bring any plugin past it. So no entry may name a Tauri family
//! crate: the root `Cargo.toml`'s `[patch.<source>]` and `[replace]`, and `[patch.<source>]` of
//! `.cargo/config.toml`, which cargo also applies and prefers over the manifest's (cargo 1.99
//! config reference, `[patch]`; the config has no `[replace]`). An entry names the package of its
//! `package` field, else of its key; a `[replace]` key is a package ID spec (`name@1.0.0`,
//! `name:1.0.0`, `<url>#name@1.0.0` or `<url>#1.0.0`). Names match with `_` read as `-`, in
//! any ASCII case. Patches of other crates stay allowed: the graph rule sees what they pull in.

use super::{Fix, is_tauri};
use crate::checks::toml_lines::{Manifest, Patch};
use crate::checks::violation;
use crate::violation::Violation;

/// Which file the entries come from: the manifest reads `[replace]`, the cargo config does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    /// The workspace's root `Cargo.toml`.
    Manifest,
    /// `.cargo/config.toml`.
    CargoConfig,
}

/// One violation per entry in the file `path` holding `text` that patches a Tauri crate, at
/// the entry's key.
pub(super) fn violations(path: &str, text: &str, source: Source) -> Vec<Violation> {
    Manifest::parse(text)
        .patches()
        .into_iter()
        .filter(|patch| source == Source::Manifest || patch.table != "[replace]")
        .filter_map(|patch| {
            let name = patched_package(&patch);
            let normal = name.to_ascii_lowercase().replace('_', "-");
            is_tauri(&normal).then(|| {
                let message = format!(
                    "`{}` swaps in another source for the Tauri crate `{name}`, whose \
                     dependencies the plugin rules trust",
                    patch.table
                );
                violation(path, patch.line, message, Fix::Patched)
            })
        })
        .collect()
}

/// The package `patch` swaps: its `package` field, else the package its key names.
fn patched_package(patch: &Patch) -> String {
    match &patch.package {
        Some(package) => package.clone(),
        None if patch.table == "[replace]" => spec_name(&patch.key).to_owned(),
        None => patch.key.clone(),
    }
}

/// The package name in the package ID spec `spec`: the part before `@` or `:`, after a URL's
/// `#`; for `<url>#<version>` or a bare URL, the URL's last path segment without `.git`.
fn spec_name(spec: &str) -> &str {
    let (url, fragment) = match spec.split_once('#') {
        Some((url, fragment)) => (Some(url), fragment),
        None if spec.contains("://") => (Some(spec), ""),
        None => (None, spec),
    };
    let name = fragment.split(['@', ':']).next().unwrap_or(fragment);
    match url {
        Some(url) if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) => {
            let segment = url.trim_end_matches('/').rsplit('/').next().unwrap_or(url);
            segment.strip_suffix(".git").unwrap_or(segment)
        }
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_names_its_package() {
        assert_eq!(spec_name("tauri:2.0.0"), "tauri");
        assert_eq!(spec_name("tauri@2.0.0"), "tauri");
        assert_eq!(spec_name("tauri"), "tauri");
        let index = "https://github.com/rust-lang/crates.io-index";
        assert_eq!(spec_name(&format!("{index}#wry@0.50.0")), "wry");
        assert_eq!(
            spec_name("https://github.com/tauri-apps/tauri#2.0.0"),
            "tauri"
        );
        assert_eq!(spec_name("https://github.com/tauri-apps/tao.git"), "tao");
    }
}
