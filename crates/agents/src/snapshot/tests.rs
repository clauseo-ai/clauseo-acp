use std::collections::BTreeSet;

use serde_json::Value;

use super::*;
use crate::platform::Libc;

fn id(text: &str) -> AgentId {
    AgentId::new(text).unwrap()
}

fn claude() -> AgentId {
    id("claude-acp")
}

fn codex() -> AgentId {
    id("codex-acp")
}

fn linux() -> Platform {
    Platform::new(Os::Linux, Arch::X64)
}

fn lock_paths(pins: &AgentPins) -> BTreeSet<String> {
    let lock: Value = serde_json::from_str(pins.lock()).unwrap();
    lock["packages"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[test]
fn the_embedded_snapshot_builds() {
    let built = build_embedded().unwrap();
    assert_eq!(built, Snapshot::embedded());
    let ids: Vec<&str> = built.agents().map(|entry| entry.id.as_str()).collect();
    assert_eq!(ids, ["claude-acp", "codex-acp"]);
    let entry = built.agent(&claude()).unwrap();
    assert_eq!(entry.display_name, "Claude");
    assert_eq!(entry.bin, "dist/index.js");
    assert_eq!(entry.install_hosts, ["nodejs.org", "registry.npmjs.org"]);
    assert_eq!(
        built.agent(&codex()).unwrap().signin_hosts,
        ["auth.openai.com", "chatgpt.com"]
    );
    assert_eq!(built.pins(&claude()).unwrap().version(), "0.89.1");
    assert_eq!(built.pins(&codex()).unwrap().version(), "2.2.2");
}

#[test]
fn done_when_2_the_embedded_pins_are_g16s_files() {
    let live = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../limits/tests/live");
    let package_json = std::fs::read_to_string(live.join("package.json")).unwrap();
    let lock = std::fs::read_to_string(live.join("package-lock.json")).unwrap();
    let snapshot = Snapshot::embedded();
    assert_eq!(snapshot.pins_package_json(), package_json);
    assert_eq!(snapshot.pins_lock(), lock);
}

#[test]
fn claude_has_115_entries_and_codex_30_plus_the_root() {
    let snapshot = Snapshot::embedded();
    let claude_pins = snapshot.pins(&claude()).unwrap();
    let codex_pins = snapshot.pins(&codex()).unwrap();
    assert_eq!(claude_pins.package_count(), 115);
    assert_eq!(lock_paths(claude_pins).len(), 116);
    assert_eq!(codex_pins.package_count(), 30);
    assert_eq!(lock_paths(codex_pins).len(), 31);
    assert_eq!(snapshot.packages(&claude(), linux()).len(), 108);
    assert_eq!(snapshot.packages(&codex(), linux()).len(), 25);
}

#[test]
fn no_entry_reachable_only_from_the_other_root_is_in_an_agents_lock() {
    let snapshot = Snapshot::embedded();
    let claude_paths = lock_paths(snapshot.pins(&claude()).unwrap());
    let codex_paths = lock_paths(snapshot.pins(&codex()).unwrap());
    let shared: BTreeSet<&String> = claude_paths.intersection(&codex_paths).collect();
    for only_codex in [
        "node_modules/@agentclientprotocol/codex-acp",
        "node_modules/@openai/codex",
    ] {
        assert!(!claude_paths.contains(only_codex), "{only_codex}");
    }
    for only_claude in [
        "node_modules/@agentclientprotocol/claude-agent-acp",
        "node_modules/@anthropic-ai/claude-agent-sdk",
        "node_modules/@anthropic-ai/sdk",
    ] {
        assert!(!codex_paths.contains(only_claude), "{only_claude}");
    }
    // What both hold is reachable from both roots: the root entry, the ACP SDK, diff, zod, and
    // cross-spawn's tree (Claude reaches it through the MCP SDK, a peer).
    let shared: Vec<&str> = shared.iter().map(|path| path.as_str()).collect();
    assert_eq!(
        shared,
        [
            "",
            "node_modules/@agentclientprotocol/sdk",
            "node_modules/cross-spawn",
            "node_modules/diff",
            "node_modules/isexe",
            "node_modules/path-key",
            "node_modules/shebang-command",
            "node_modules/shebang-regex",
            "node_modules/which",
            "node_modules/zod",
        ]
    );
    assert!(
        !claude_paths
            .iter()
            .any(|path| path.starts_with("node_modules/@openai/"))
    );
    let lock: Value = serde_json::from_str(snapshot.pins(&claude()).unwrap().lock()).unwrap();
    assert_eq!(
        lock["packages"][""]["dependencies"],
        serde_json::json!({"@agentclientprotocol/claude-agent-acp": "0.89.1"})
    );
}

#[test]
fn codexs_package_json_has_the_override_and_claudes_does_not() {
    let snapshot = Snapshot::embedded();
    let codex_manifest: Value =
        serde_json::from_str(snapshot.pins(&codex()).unwrap().package_json()).unwrap();
    assert_eq!(
        codex_manifest["overrides"],
        serde_json::json!({"@openai/codex": "0.162.1"})
    );
    assert_eq!(
        codex_manifest["dependencies"],
        serde_json::json!({"@agentclientprotocol/codex-acp": "2.2.2"})
    );
    let claude_json = snapshot.pins(&claude()).unwrap().package_json();
    assert!(!claude_json.contains("overrides"), "{claude_json}");
}

#[test]
fn every_embedded_resolved_starts_with_the_registry_base() {
    let lock: Value = serde_json::from_str(PINS_LOCK).unwrap();
    let packages = lock["packages"].as_object().unwrap();
    assert_eq!(packages.len(), 137);
    for (path, entry) in packages {
        if path.is_empty() {
            continue;
        }
        let resolved = entry["resolved"].as_str().unwrap();
        assert!(resolved.starts_with(NPM_REGISTRY), "{path}: {resolved}");
        assert!(
            entry["integrity"].as_str().unwrap().starts_with("sha512-"),
            "{path}"
        );
    }
}

#[test]
fn the_sizes_stamp_is_the_current_locks_hash() {
    let current = short_hash(&[PINS_LOCK.as_bytes()]);
    assert_eq!(
        SIZES_STAMP, current,
        "G16's lock changed: measure each agent's download and unpacked bytes per platform \
         again, update SIZES in snapshot/embedded.rs, then set SIZES_STAMP to {current}"
    );
}

#[test]
fn the_hash_differs_per_agent_and_per_platform() {
    let snapshot = Snapshot::embedded();
    let mut platforms: Vec<Platform> = Platform::ALL.to_vec();
    platforms.push(Platform {
        libc: Some(Libc::Musl),
        ..linux()
    });
    let mut seen = BTreeSet::new();
    for agent in [claude(), codex()] {
        for platform in &platforms {
            let hash = snapshot.install_hash(&agent, *platform).unwrap();
            assert_eq!(hash.len(), 16);
            assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert!(seen.insert(hash), "{agent} {platform}");
        }
    }
    assert_eq!(snapshot.install_hash(&id("other"), linux()), None);
}

#[test]
fn reserved_ids_are_refused() {
    let snapshot = Snapshot::embedded();
    for reserved in RESERVED_IDS {
        let entry = AgentEntry::new(id(reserved), "X", "@agentclientprotocol/codex-acp", "x.js");
        let result = Snapshot::new(
            PINS_PACKAGE_JSON,
            PINS_LOCK,
            NPM_REGISTRY,
            snapshot.node().clone(),
            vec![entry],
        );
        assert_eq!(result, Err(SnapshotError::ReservedId(reserved.to_owned())));
    }
}

#[test]
fn a_root_not_in_package_json_and_a_foreign_registry_are_refused() {
    let node = Snapshot::embedded().node().clone();
    let entry = AgentEntry::new(id("x"), "X", "left-pad", "x.js");
    let unknown = Snapshot::new(
        PINS_PACKAGE_JSON,
        PINS_LOCK,
        NPM_REGISTRY,
        node.clone(),
        vec![entry],
    );
    assert!(matches!(
        unknown,
        Err(SnapshotError::Pins {
            source: PinsError::UnknownRoot(_),
            ..
        })
    ));
    let entry = AgentEntry::new(id("x"), "X", "@agentclientprotocol/codex-acp", "x.js");
    let foreign = Snapshot::new(
        PINS_PACKAGE_JSON,
        PINS_LOCK,
        "https://mirror.example/",
        node,
        vec![entry],
    );
    assert!(matches!(
        foreign,
        Err(SnapshotError::Pins {
            source: PinsError::Resolved { .. },
            ..
        })
    ));
}

#[test]
fn sizes_follow_the_records_formula() {
    let snapshot = Snapshot::embedded();
    let mac = Platform::new(Os::Darwin, Arch::Arm64);
    assert_eq!(
        snapshot.download_bytes(&claude(), mac, false),
        Some(112_272_099)
    );
    assert_eq!(
        snapshot.download_bytes(&claude(), mac, true),
        Some(112_272_099 + 53_049_576)
    );
    let base: u64 = 2 * 112_272_099 + 284_447_720;
    assert_eq!(
        snapshot.need_bytes(&claude(), mac, false),
        Some(base + base.div_ceil(10))
    );
    let with_node = base + 53_049_576 + 197_365_224;
    assert_eq!(
        snapshot.need_bytes(&claude(), mac, true),
        Some(with_node + with_node.div_ceil(10))
    );
    let musl = Platform {
        libc: Some(Libc::Musl),
        ..linux()
    };
    assert_eq!(
        snapshot.download_bytes(&codex(), musl, false),
        Some(167_703_509)
    );
    assert_eq!(snapshot.download_bytes(&id("other"), musl, false), None);
}

#[test]
fn a_fixture_with_no_sizes_row_counts_only_node() {
    let snapshot = Snapshot::embedded();
    let entry = AgentEntry::new(
        id("x"),
        "X",
        "@agentclientprotocol/codex-acp",
        "dist/index.js",
    );
    let fixture = Snapshot::new(
        PINS_PACKAGE_JSON,
        PINS_LOCK,
        NPM_REGISTRY,
        snapshot.node().clone(),
        vec![entry],
    )
    .unwrap();
    assert_eq!(fixture.download_bytes(&id("x"), linux(), false), Some(0));
    assert_eq!(
        fixture.download_bytes(&id("x"), linux(), true),
        Some(58_256_361)
    );
    assert_eq!(fixture.need_bytes(&id("x"), linux(), false), Some(0));
}

#[test]
fn every_platform_has_its_node_archive() {
    let snapshot = Snapshot::embedded();
    for platform in Platform::ALL {
        let archive = snapshot.node_archive(platform).unwrap();
        let stem = format!("node-v24.22.0-{}", platform.node_name());
        assert!(archive.name().starts_with(&stem), "{}", archive.name());
        assert_eq!(archive.is_zip(), platform.os == Os::Win32);
        assert_eq!(archive.sha256_hex().len(), 64);
    }
    let linux_archive = snapshot.node_archive(linux()).unwrap();
    assert_eq!(
        snapshot.node().url(linux_archive),
        "https://nodejs.org/dist/v24.22.0/node-v24.22.0-linux-x64.tar.gz"
    );
    assert_eq!(
        linux_archive.sha256_hex(),
        "28ea0a438de9a13bd7891fd758eee6c7819ee8af5442dfd8d45bd65e56d4228c"
    );
}
