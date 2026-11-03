//! Each agent's derived pins: from G16's `package.json` and `package-lock.json`
//! (lockfileVersion 3), a `package.json` with the agent's one root dependency and a lock with
//! every `packages` entry reachable from it, written as canonical JSON.
//!
//! Reachable means through `dependencies`, `optionalDependencies` and `peerDependencies`, each
//! name resolved the way npm's `node_modules` lookup does: for an entry at path `p`, first
//! `p/node_modules/<name>`, then each ancestor's `node_modules/<name>`, up to the top-level
//! `node_modules/<name>`. An optional peer the lock lacks is skipped; any other missing
//! dependency is an error. Entries are copied whole: paths, versions, `resolved`, `integrity`
//! and npm's flags.
//!
//! Canonical JSON (keys sorted at every level, two-space indent, a trailing newline) makes the
//! bytes, and so the install `<hash>`, the same whether or not a build turns on `serde_json`'s
//! `preserve_order`. Nothing here relies on a `Map`'s order.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::integrity::Expected;
use crate::platform::Platform;

/// Why the pins did not derive.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PinsError {
    /// A file is not JSON, or not the shape npm writes.
    #[error("the pins do not read: {0}")]
    Malformed(String),
    /// The root package is not a dependency in `package.json`.
    #[error("{0} is not a dependency in the pins' package.json")]
    UnknownRoot(String),
    /// A dependency has no entry the lookup finds, and is not an optional peer.
    #[error("{name}, a dependency of {from:?}, is not in the lock")]
    Missing {
        /// The entry's path that depends on it.
        from: String,
        /// The dependency's name.
        name: String,
    },
    /// An entry's `resolved` is missing or does not start with the registry base.
    #[error("{path}'s resolved URL {resolved:?} is not under the registry {registry}")]
    Resolved {
        /// The entry's path.
        path: String,
        /// Its `resolved`, empty when missing.
        resolved: String,
        /// The registry base.
        registry: String,
    },
    /// An entry's `integrity` is missing or not one `sha512-<base64>`.
    #[error("{path}'s integrity is not one sha512 digest")]
    Integrity {
        /// The entry's path.
        path: String,
    },
}

/// One `packages` entry of a derived lock, as a download needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockedPackage {
    /// The entry's path, `node_modules/<name>` or nested.
    pub(crate) path: String,
    /// The package's name: the entry's `name` (an alias names its real package), else the
    /// path's last `node_modules/` part.
    pub(crate) name: String,
    /// Its version.
    pub(crate) version: String,
    /// Its tarball URL, under the registry base.
    pub(crate) resolved: String,
    /// Its SHA-512, from `integrity`.
    pub(crate) integrity: Expected,
    /// npm's `os` list; empty allows all.
    pub(crate) os: Vec<String>,
    /// npm's `cpu` list.
    pub(crate) cpu: Vec<String>,
    /// npm's `libc` list.
    pub(crate) libc: Vec<String>,
    /// npm's `optional` flag.
    pub(crate) optional: bool,
}

impl LockedPackage {
    /// Whether npm installs this entry on `platform` (its `os`, `cpu` and `libc` rule).
    pub(crate) fn allowed_on(&self, platform: Platform) -> bool {
        platform.allows(&self.os, &self.cpu, &self.libc)
    }
}

/// One agent's derived pair and its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPins {
    package_json: String,
    lock: String,
    version: String,
    packages: Vec<LockedPackage>,
}

impl AgentPins {
    /// The derived `package.json`, canonical JSON.
    pub fn package_json(&self) -> &str {
        &self.package_json
    }

    /// The derived `package-lock.json`, canonical JSON.
    pub fn lock(&self) -> &str {
        &self.lock
    }

    /// The adapter's version: the root package's entry in the lock.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// How many `packages` entries the lock holds, the root entry not counted.
    pub fn package_count(&self) -> usize {
        self.packages.len()
    }

    /// Every entry but the root, by path.
    pub(crate) fn packages(&self) -> &[LockedPackage] {
        &self.packages
    }

    /// The entries npm installs on `platform`, by path.
    pub(crate) fn packages_for(&self, platform: Platform) -> Vec<&LockedPackage> {
        self.packages
            .iter()
            .filter(|package| package.allowed_on(platform))
            .collect()
    }
}

/// Derives the pair for `root` from G16's two files. Overrides in `package.json` are kept only
/// for a package name the derived lock holds, so `@openai/codex`'s goes to Codex alone.
pub(crate) fn derive(
    package_json: &str,
    lock: &str,
    root: &str,
    registry: &str,
) -> Result<AgentPins, PinsError> {
    let manifest: Value = parse(package_json, "package.json")?;
    let lock: Value = parse(lock, "package-lock.json")?;
    let range = manifest
        .get("dependencies")
        .and_then(|deps| deps.get(root))
        .ok_or_else(|| PinsError::UnknownRoot(root.to_owned()))?
        .clone();
    if lock.get("lockfileVersion").and_then(Value::as_u64) != Some(3) {
        return Err(malformed("package-lock.json is not lockfileVersion 3"));
    }
    let entries = lock
        .get("packages")
        .and_then(Value::as_object)
        .ok_or_else(|| malformed("package-lock.json has no packages"))?;
    let kept = reachable(entries, root)?;

    let mut packages = Vec::with_capacity(kept.len());
    let mut derived = Map::new();
    for path in &kept {
        let entry = entries
            .get(path)
            .ok_or_else(|| malformed("a kept path vanished"))?;
        packages.push(locked(path, entry, registry)?);
        derived.insert(path.clone(), entry.clone());
    }
    let mut root_entry = entries
        .get("")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| malformed("package-lock.json has no root entry"))?;
    let mut one_dependency = Map::new();
    one_dependency.insert(root.to_owned(), range);
    root_entry.insert("dependencies".into(), Value::Object(one_dependency.clone()));
    derived.insert(String::new(), Value::Object(root_entry));

    let mut derived_lock = Map::new();
    for key in ["name", "version", "lockfileVersion", "requires"] {
        if let Some(value) = lock.get(key) {
            derived_lock.insert(key.to_owned(), value.clone());
        }
    }
    derived_lock.insert("packages".into(), Value::Object(derived));

    let mut derived_manifest = Map::new();
    for key in ["name", "version", "private", "license"] {
        if let Some(value) = manifest.get(key) {
            derived_manifest.insert(key.to_owned(), value.clone());
        }
    }
    derived_manifest.insert("dependencies".into(), Value::Object(one_dependency));
    let names: BTreeSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    if let Some(overrides) = manifest.get("overrides").and_then(Value::as_object) {
        let kept_overrides: Map<String, Value> = overrides
            .iter()
            .filter(|(key, _)| names.contains(override_name(key)))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if !kept_overrides.is_empty() {
            derived_manifest.insert("overrides".into(), Value::Object(kept_overrides));
        }
    }

    let version = packages
        .iter()
        .find(|p| p.path == format!("node_modules/{root}"))
        .map(|p| p.version.clone())
        .unwrap_or_default();
    Ok(AgentPins {
        package_json: canonical(&Value::Object(derived_manifest)),
        lock: canonical(&Value::Object(derived_lock)),
        version,
        packages,
    })
}

/// The paths reachable from `node_modules/<root>`, sorted.
fn reachable(entries: &Map<String, Value>, root: &str) -> Result<BTreeSet<String>, PinsError> {
    let start = format!("node_modules/{root}");
    if !entries.contains_key(&start) {
        return Err(PinsError::Missing {
            from: String::new(),
            name: root.to_owned(),
        });
    }
    let mut kept = BTreeSet::new();
    let mut queue = vec![start];
    while let Some(path) = queue.pop() {
        if !kept.insert(path.clone()) {
            continue;
        }
        let entry = entries
            .get(&path)
            .ok_or_else(|| malformed("a queued path vanished"))?;
        let optional_peer = |name: &str| {
            entry
                .get("peerDependenciesMeta")
                .and_then(|meta| meta.get(name))
                .and_then(|meta| meta.get("optional"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        };
        for (field, peer) in [
            ("dependencies", false),
            ("optionalDependencies", false),
            ("peerDependencies", true),
        ] {
            let Some(deps) = entry.get(field).and_then(Value::as_object) else {
                continue;
            };
            let mut names: Vec<&String> = deps.keys().collect();
            names.sort();
            for name in names {
                match resolve(entries, &path, name) {
                    Some(found) => queue.push(found),
                    None if peer && optional_peer(name) => {}
                    None => {
                        return Err(PinsError::Missing {
                            from: path.clone(),
                            name: name.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(kept)
}

/// npm's lookup of `name` from the entry at `from`: `from/node_modules/name`, then each
/// ancestor's, then the top level's.
fn resolve(entries: &Map<String, Value>, from: &str, name: &str) -> Option<String> {
    let mut base = from;
    loop {
        let candidate = if base.is_empty() {
            format!("node_modules/{name}")
        } else {
            format!("{base}/node_modules/{name}")
        };
        if entries.contains_key(&candidate) {
            return Some(candidate);
        }
        if base.is_empty() {
            return None;
        }
        base = base.rfind("/node_modules/").map_or("", |at| &base[..at]);
    }
}

/// The checked [`LockedPackage`] for the entry at `path`.
fn locked(path: &str, entry: &Value, registry: &str) -> Result<LockedPackage, PinsError> {
    let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_owned);
    let list = |key: &str| -> Vec<String> {
        entry
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let resolved = text("resolved").unwrap_or_default();
    if !resolved.starts_with(registry) || resolved.len() == registry.len() {
        return Err(PinsError::Resolved {
            path: path.to_owned(),
            resolved,
            registry: registry.to_owned(),
        });
    }
    let integrity = text("integrity")
        .and_then(|integrity| Expected::from_integrity(&integrity).ok())
        .ok_or_else(|| PinsError::Integrity {
            path: path.to_owned(),
        })?;
    let from_path = path
        .rfind("node_modules/")
        .map_or(path, |at| &path[at + "node_modules/".len()..]);
    Ok(LockedPackage {
        path: path.to_owned(),
        name: text("name").unwrap_or_else(|| from_path.to_owned()),
        version: text("version").unwrap_or_default(),
        resolved,
        integrity,
        os: list("os"),
        cpu: list("cpu"),
        libc: list("libc"),
        optional: entry
            .get("optional")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// The package an `overrides` key names: `name` from `name` or `name@range`.
fn override_name(key: &str) -> &str {
    let scoped = usize::from(key.starts_with('@'));
    key.get(scoped..)
        .and_then(|rest| rest.find('@'))
        .map_or(key, |at| &key[..at + scoped])
}

/// `text` as JSON, or [`PinsError::Malformed`] naming `file`.
fn parse(text: &str, file: &str) -> Result<Value, PinsError> {
    serde_json::from_str(text).map_err(|err| malformed(&format!("{file}: {err}")))
}

/// A [`PinsError::Malformed`].
fn malformed(what: &str) -> PinsError {
    PinsError::Malformed(what.to_owned())
}

/// `value` as canonical JSON: keys sorted at every level, two-space indent, `": "` between a
/// key and its value, empty objects and arrays as `{}` and `[]`, and a trailing newline.
pub(crate) fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out.push('\n');
    out
}

/// Appends `value` at nesting `depth`.
fn write_value(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            out.push('{');
            for (n, (key, item)) in sorted.into_iter().enumerate() {
                out.push_str(if n == 0 { "\n" } else { ",\n" });
                indent(out, depth + 1);
                out.push_str(&Value::String(key.clone()).to_string());
                out.push_str(": ");
                write_value(out, item, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
        Value::Array(items) if !items.is_empty() => {
            out.push('[');
            for (n, item) in items.iter().enumerate() {
                out.push_str(if n == 0 { "\n" } else { ",\n" });
                indent(out, depth + 1);
                write_value(out, item, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        Value::Object(_) => out.push_str("{}"),
        Value::Array(_) => out.push_str("[]"),
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// Appends two spaces per level.
fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests;
