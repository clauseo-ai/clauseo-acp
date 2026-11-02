//! The crate's own dependency tree holds no async runtime, no HTTP stack built on one, no Tauri
//! and no Clauseo host: it reads `cargo metadata` (offline, for this machine's target) and walks
//! the normal and build edges from `clauseo-connectors`. Dev-dependencies do not count.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

/// Banned by name prefix: `tokio`, `tokio-util`, `hyper-util`, `tauri-plugin-*` ...
#[cfg(test)]
const BANNED_PREFIXES: [&str; 3] = ["tokio", "hyper", "tauri"];

/// Banned by exact name.
#[cfg(test)]
const BANNED_NAMES: [&str; 5] = ["async-std", "smol", "async-io", "reqwest", "clauseo-host"];

/// Whether a package name is banned from the crate's tree.
#[cfg(test)]
fn banned(name: &str) -> bool {
    BANNED_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || BANNED_NAMES.contains(&name)
}

#[test]
fn the_ban_matches_prefixes_and_names() {
    for name in [
        "tokio",
        "tokio-util",
        "hyper",
        "hyper-util",
        "tauri",
        "tauri-plugin-deep-link",
        "async-std",
        "smol",
        "async-io",
        "reqwest",
        "clauseo-host",
    ] {
        assert!(banned(name), "{name}");
    }
    for name in [
        "ureq",
        "rustls",
        "async-trait",
        "smoltcp",
        "clauseo-protocol",
        "http",
    ] {
        assert!(!banned(name), "{name}");
    }
}

/// Runs `cargo` (the one that runs this test) and returns its stdout as JSON.
#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test reads cargo's own output; the crate starts no other process"
)]
fn cargo_json(args: &[&str]) -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = std::process::Command::new(cargo)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// This machine's target triple, from `rustc -vV` through cargo's own rustc.
#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test reads rustc's own output; the crate starts no other process"
)]
fn host_triple() -> String {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = std::process::Command::new(rustc)
        .arg("-vV")
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap()
        .to_owned()
}

#[test]
fn no_async_runtime_tauri_or_host_in_the_tree() {
    let triple = host_triple();
    let metadata = cargo_json(&[
        "metadata",
        "--format-version",
        "1",
        "--offline",
        "--locked",
        "--filter-platform",
        &triple,
    ]);
    let names: HashMap<&str, &str> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| {
            (
                package["id"].as_str().unwrap(),
                package["name"].as_str().unwrap(),
            )
        })
        .collect();
    let nodes: HashMap<&str, &Value> = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| (node["id"].as_str().unwrap(), node))
        .collect();
    let root = names
        .iter()
        .find(|(_, name)| **name == "clauseo-connectors")
        .map(|(id, _)| *id)
        .unwrap();
    let mut seen = BTreeSet::new();
    let mut queue = vec![root];
    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        for dep in nodes[id]["deps"].as_array().unwrap() {
            let normal_or_build = dep["dep_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind["kind"].as_str() != Some("dev"));
            if normal_or_build {
                queue.push(dep["pkg"].as_str().unwrap());
            }
        }
    }
    let tree: BTreeSet<&str> = seen.iter().map(|id| names[id]).collect();
    assert!(
        tree.contains("ureq") && tree.contains("rustls"),
        "the walk found the tree: {tree:?}"
    );
    let found: Vec<&str> = tree.iter().copied().filter(|name| banned(name)).collect();
    assert!(
        found.is_empty(),
        "banned in clauseo-connectors' tree: {found:?}"
    );
}
