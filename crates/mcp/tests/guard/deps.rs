//! The guard's dependency tree, as the workspace's `Cargo.lock` pins it: no HTTP client, socket or
//! TLS crate, async runtime or tauri may be reachable through a dependency of `clauseo-mcp`, on any
//! target. The test reads the lockfile and runs no cargo, so it needs no network and no downloads.
//!
//! The lockfile merges dependency kinds and lists every target's packages, so the walk is a
//! superset of what ships. That is stricter, which suits a ban.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Packages that must never be reachable from `clauseo-mcp`.
const BANNED: &[&str] = &[
    "reqwest",
    "hyper",
    "ureq",
    "curl",
    "rustls",
    "native-tls",
    "tauri",
    "tokio",
    "socket2",
    "mio",
    "h2",
    "hyper-util",
    "openssl",
    "isahc",
    "attohttpc",
    "async-std",
    "smol",
];

/// One `[[package]]` block of `Cargo.lock`.
#[derive(Debug, PartialEq, Eq)]
struct Package {
    name: String,
    version: String,
    /// The `dependencies` entries as written: `"name"`, `"name version"` or
    /// `"name version (source)"`.
    deps: Vec<String>,
}

/// The double-quoted strings in `text`.
fn quoted(text: &str) -> Vec<String> {
    text.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// The first quoted value of a `key = "value"` line.
fn value_of(line: &str, key: &str) -> Option<String> {
    let value = line.strip_prefix(key)?.strip_prefix(" = ")?;
    quoted(value).into_iter().next()
}

/// The `[[package]]` blocks of a `Cargo.lock`. Other tables are skipped.
fn parse_lock(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut current: Option<Package> = None;
    // Inside a `dependencies = [` array, whose quoted entries end at a line holding `]`.
    let mut in_deps = false;
    for line in text.lines().map(str::trim) {
        if in_deps {
            if let Some(pkg) = current.as_mut() {
                pkg.deps.extend(quoted(line));
            }
            in_deps = !line.contains(']');
        } else if line == "[[package]]" {
            packages.extend(current.take());
            current = Some(Package {
                name: String::new(),
                version: String::new(),
                deps: Vec::new(),
            });
        } else if line.starts_with('[') {
            packages.extend(current.take());
        } else if let Some(pkg) = current.as_mut() {
            if let Some(rest) = line.strip_prefix("dependencies = [") {
                pkg.deps.extend(quoted(rest));
                in_deps = !rest.contains(']');
            } else if let Some(name) = value_of(line, "name") {
                pkg.name = name;
            } else if let Some(version) = value_of(line, "version") {
                pkg.version = version;
            }
        }
    }
    packages.extend(current);
    packages
}

/// The index of the package a dependency entry names. An entry with a version names that version;
/// one without names the package's only version.
fn resolve(packages: &[Package], entry: &str) -> usize {
    let mut words = entry.split_whitespace();
    let name = words.next().unwrap_or_default();
    let version = words.next().filter(|word| !word.starts_with('('));
    packages
        .iter()
        .position(|pkg| pkg.name == name && version.is_none_or(|v| pkg.version == v))
        .unwrap_or_else(|| panic!("Cargo.lock has no package for the entry `{entry}`"))
}

/// Every package reachable from `clauseo-mcp` through the lockfile's dependency lists, by name,
/// each with the shortest chain that reaches it.
fn reachable(lock: &str) -> BTreeMap<String, Vec<String>> {
    let packages = parse_lock(lock);
    let root = packages
        .iter()
        .position(|pkg| pkg.name == "clauseo-mcp")
        .expect("Cargo.lock has the clauseo-mcp package");
    let mut found = BTreeMap::new();
    let mut seen = BTreeSet::from([root]);
    let mut queue = VecDeque::from([(root, vec!["clauseo-mcp".to_owned()])]);
    while let Some((index, chain)) = queue.pop_front() {
        let Some(pkg) = packages.get(index) else {
            continue;
        };
        for entry in &pkg.deps {
            let dep = resolve(&packages, entry);
            if seen.insert(dep) {
                let name = packages[dep].name.clone();
                let mut chain = chain.clone();
                chain.push(name.clone());
                found.entry(name).or_insert_with(|| chain.clone());
                queue.push_back((dep, chain));
            }
        }
    }
    found
}

#[test]
fn no_http_client_tls_runtime_or_tauri_is_reachable() {
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
        .expect("the workspace Cargo.lock is readable");
    let found = reachable(&lock);
    assert!(
        found.contains_key("serde_json"),
        "the walk saw the one direct dependency"
    );
    assert!(
        found.contains_key("serde"),
        "the walk went past the first edge"
    );
    let banned: Vec<String> = BANNED
        .iter()
        .filter_map(|name| found.get(*name).map(|chain| chain.join(" -> ")))
        .collect();
    assert!(banned.is_empty(), "banned packages reachable: {banned:?}");
}

#[test]
fn lockfile_parser_keeps_each_version_and_resolves_entries_to_them() {
    // `dup` has two versions, so its entries carry one. `clauseo-mcp` reaches `dup 1.0.0`
    // directly; `dup 2.0.0` is reachable only through `leaf`, which names that version.
    let lock = r#"
version = 4

[[package]]
name = "clauseo-mcp"
version = "0.0.0"
dependencies = [
 "dup 1.0.0 (registry+https://example.test/index)",
 "serde_json",
]

[[package]]
name = "dup"
version = "1.0.0"
source = "registry+https://example.test/index"
dependencies = [
 "leaf",
]

[[package]]
name = "dup"
version = "2.0.0"
source = "registry+https://example.test/index"
dependencies = ["tokio"]

[[package]]
name = "leaf"
version = "0.1.0"
source = "registry+https://example.test/index"
dependencies = [
 "dup 2.0.0 (registry+https://example.test/index)",
]

[[package]]
name = "serde_json"
version = "1.0.0"

[[package]]
name = "tokio"
version = "1.0.0"

[metadata]
"checksum tokio 1.0.0" = "unused"
"#;
    let packages = parse_lock(lock);
    let versions: Vec<(&str, &str)> = packages
        .iter()
        .map(|pkg| (pkg.name.as_str(), pkg.version.as_str()))
        .collect();
    assert_eq!(
        versions,
        [
            ("clauseo-mcp", "0.0.0"),
            ("dup", "1.0.0"),
            ("dup", "2.0.0"),
            ("leaf", "0.1.0"),
            ("serde_json", "1.0.0"),
            ("tokio", "1.0.0"),
        ]
    );
    assert_eq!(packages[2].deps, ["tokio"]);

    let found = reachable(lock);
    let chain = |name: &str| found.get(name).map(|chain| chain.join(" -> "));
    assert_eq!(chain("leaf").as_deref(), Some("clauseo-mcp -> dup -> leaf"));
    assert_eq!(
        chain("tokio").as_deref(),
        Some("clauseo-mcp -> dup -> leaf -> dup -> tokio")
    );
    assert_eq!(
        chain("serde_json").as_deref(),
        Some("clauseo-mcp -> serde_json")
    );
    assert!(!found.contains_key("clauseo-mcp"));
}
