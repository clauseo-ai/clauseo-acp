//! The guard's dependency tree, as cargo resolves it: no HTTP client, TLS stack, async runtime or
//! tauri may be reachable through a normal or build dependency, on any target.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::Value;

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
];

/// `cargo metadata` for the workspace, offline (tests never reach the network) and locked.
#[expect(
    clippy::disallowed_methods,
    reason = "the test reads cargo's own resolution of the dependency graph"
)]
fn metadata() -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .arg("--manifest-path")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Every package reachable from `clauseo-mcp` through normal or build edges, by name, each with
/// the chain that reaches it.
fn reachable(metadata: &Value) -> BTreeMap<String, Vec<String>> {
    let name_of: BTreeMap<&str, &str> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p["name"].as_str().unwrap()))
        .collect();
    let edges: BTreeMap<&str, Vec<&str>> = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            let deps = node["deps"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|dep| {
                    dep["dep_kinds"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|kind| kind["kind"].as_str() != Some("dev"))
                })
                .map(|dep| dep["pkg"].as_str().unwrap())
                .collect();
            (node["id"].as_str().unwrap(), deps)
        })
        .collect();
    let root = name_of
        .iter()
        .find(|(_, name)| **name == "clauseo-mcp")
        .map(|(id, _)| *id)
        .unwrap();
    let mut found = BTreeMap::new();
    let mut seen = BTreeSet::from([root]);
    let mut queue = vec![(root, vec!["clauseo-mcp".to_owned()])];
    while let Some((id, chain)) = queue.pop() {
        for dep in edges.get(id).into_iter().flatten() {
            if seen.insert(*dep) {
                let mut chain = chain.clone();
                chain.push(name_of[dep].to_owned());
                found.insert(name_of[dep].to_owned(), chain.clone());
                queue.push((*dep, chain));
            }
        }
    }
    found
}

#[test]
fn no_http_client_tls_runtime_or_tauri_is_reachable() {
    let found = reachable(&metadata());
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
