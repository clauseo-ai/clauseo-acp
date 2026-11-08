use std::time::Duration;

use super::*;
use crate::platform::{Arch, Os};
use crate::write::TestDir;

const HASH: &str = "0123456789abcdef";

fn agent() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn linux() -> Platform {
    Platform::new(Os::Linux, Arch::X64)
}

fn receipt(hash: &str) -> Receipt {
    Receipt::new(
        &agent(),
        "0.89.1",
        hash,
        Path::new("/usr/bin/node"),
        "v24.22.0",
        linux(),
        UNIX_EPOCH + Duration::from_secs(1_790_000_000),
    )
}

/// An install folder `name` under the agent's `install/`, with `receipt` written if given.
fn folder(data: &TestDir, name: &str, receipt: Option<&Receipt>) -> PathBuf {
    let path = install_folder(&data.0, &agent(), name);
    write::create_dir_all(&path.join("node_modules")).unwrap();
    if let Some(receipt) = receipt {
        write_receipt(&path, receipt).unwrap();
    }
    path
}

#[test]
fn the_layout_is_the_records() {
    let data = Path::new("/data");
    let folder = install_folder(data, &agent(), HASH);
    assert_eq!(
        folder,
        data.join("agents")
            .join("claude-acp")
            .join("install")
            .join(HASH)
    );
    let entry = AgentEntry::new(
        agent(),
        "Claude",
        "@agentclientprotocol/claude-agent-acp",
        "dist/index.js",
    );
    assert_eq!(
        script_path(&folder, &entry),
        folder
            .join("node_modules")
            .join("@agentclientprotocol")
            .join("claude-agent-acp")
            .join("dist")
            .join("index.js")
    );
}

#[test]
fn a_receipt_round_trips_and_is_checked_against_agent_hash_and_platform() {
    let data = TestDir::new("receipt");
    let path = folder(&data, HASH, Some(&receipt(HASH)));
    let read = read_receipt(&path, &agent(), HASH, linux()).unwrap();
    assert_eq!(read, receipt(HASH));
    assert_eq!(read.installed_at, 1_790_000_000);
    assert_eq!(read.platform, "linux-x64-glibc");
    let text = std::fs::read_to_string(path.join(RECEIPT)).unwrap();
    assert!(text.contains("\"schema\": 1"), "{text}");
    assert!(!path.join(".clauseo-receipt.json.tmp").exists());

    let codex = AgentId::new("codex-acp").unwrap();
    assert_eq!(read_receipt(&path, &codex, HASH, linux()), None);
    assert_eq!(
        read_receipt(&path, &agent(), "fedcba9876543210", linux()),
        None
    );
    let mac = Platform::new(Os::Darwin, Arch::Arm64);
    assert_eq!(read_receipt(&path, &agent(), HASH, mac), None);
}

#[test]
fn a_malformed_or_other_schema_receipt_is_not_valid() {
    let data = TestDir::new("receipt-bad");
    let path = folder(&data, HASH, None);
    assert_eq!(read_receipt(&path, &agent(), HASH, linux()), None);
    write::write_atomic(&path.join(RECEIPT), b"{not json").unwrap();
    assert_eq!(read_receipt(&path, &agent(), HASH, linux()), None);
    let mut future = receipt(HASH);
    future.schema = 2;
    write_receipt(&path, &future).unwrap();
    assert_eq!(read_receipt(&path, &agent(), HASH, linux()), None);
}

#[test]
fn the_scan_keeps_the_current_install_and_removes_the_rest() {
    let data = TestDir::new("scan");
    let current = folder(&data, HASH, Some(&receipt(HASH)));
    let old = folder(
        &data,
        "aaaaaaaaaaaaaaaa",
        Some(&receipt("aaaaaaaaaaaaaaaa")),
    );
    let tmp = folder(&data, &format!(".tmp-{HASH}-1"), None);
    let stray = data.file("agents/claude-acp/install/.tmp-file", b"x");
    let found = scan(&data.0, &agent(), HASH, linux()).unwrap();
    assert_eq!(found.keep, Some((current.clone(), receipt(HASH))));
    let mut want = vec![old, tmp, stray];
    want.sort();
    assert_eq!(found.remove, want);
    remove_stale(&found).unwrap();
    let left: Vec<PathBuf> = std::fs::read_dir(install_root(&data.0, &agent()))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(left, [current]);
}

#[test]
fn the_current_hash_without_a_valid_receipt_is_removed() {
    let data = TestDir::new("scan-incomplete");
    let incomplete = folder(&data, HASH, None);
    let found = scan(&data.0, &agent(), HASH, linux()).unwrap();
    assert_eq!(found.keep, None);
    assert_eq!(found.remove, [incomplete]);
    let other_platform = folder(&data, HASH, Some(&receipt(HASH)));
    let mac = Platform::new(Os::Darwin, Arch::Arm64);
    assert_eq!(
        scan(&data.0, &agent(), HASH, mac).unwrap().remove,
        [other_platform]
    );
}

#[test]
fn a_missing_install_folder_is_an_empty_scan() {
    let data = TestDir::new("scan-none");
    assert_eq!(
        scan(&data.0, &agent(), HASH, linux()).unwrap(),
        Scan::default()
    );
}

#[cfg(unix)]
#[test]
fn a_link_in_install_is_removed_without_touching_its_target() {
    let data = TestDir::new("scan-link");
    let target = data.file("elsewhere/keep.txt", b"x");
    let root = install_root(&data.0, &agent());
    write::create_dir_all(&root).unwrap();
    write::symlink(target.parent().unwrap(), &root.join(HASH)).unwrap();
    let found = scan(&data.0, &agent(), HASH, linux()).unwrap();
    assert_eq!(found.remove, [root.join(HASH)]);
    remove_stale(&found).unwrap();
    assert!(target.exists());
    assert!(std::fs::symlink_metadata(root.join(HASH)).is_err());
}

/// A link at `<data>/<place>` to a folder outside the data folder: the scan and removal at start
/// fail, remove nothing, and the folder it points at keeps everything in it.
#[cfg(unix)]
fn a_link_on_the_way_to_install(place: &str) {
    let data = TestDir::new("scan-linked");
    let outside = TestDir::new("scan-linked-canary");
    let canary = outside.file("brief.docx", b"privileged");
    let nested = outside.file("exhibits/a.pdf", b"exhibit");
    // What `install/` would hold were the link at `agents` or `agents/<id>`.
    let held = outside.file("claude-acp/install/old/held.txt", b"held");
    let held_too = outside.file("install/old/held.txt", b"held");
    let link = place
        .split('/')
        .fold(data.0.clone(), |path, part| path.join(part));
    write::create_dir_all(link.parent().unwrap()).unwrap();
    write::symlink(&outside.0, &link).unwrap();
    // As the worker does at start.
    let result = scan(&data.0, &agent(), HASH, linux()).and_then(|found| remove_stale(&found));
    for path in [&canary, &nested, &held, &held_too] {
        assert!(path.is_file(), "{} was removed", path.display());
    }
    let err = result.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
}

#[cfg(unix)]
#[test]
fn a_link_at_agents_is_never_scanned() {
    a_link_on_the_way_to_install("agents");
}

#[cfg(unix)]
#[test]
fn a_link_at_the_agents_folder_is_never_scanned() {
    a_link_on_the_way_to_install("agents/claude-acp");
}

#[cfg(unix)]
#[test]
fn a_link_at_install_is_never_scanned() {
    a_link_on_the_way_to_install("agents/claude-acp/install");
}

#[test]
fn a_receipt_written_before_its_list_and_stamp_still_reads() {
    let data = TestDir::new("receipt-old");
    let mut old = serde_json::to_value(receipt(HASH)).unwrap();
    assert!(old.get("files").is_none() && old.get("node_stamp").is_none());
    old["installed_at"] = 1.into();
    let dir = folder(&data, HASH, None);
    write::write_atomic(&dir.join(RECEIPT), &serde_json::to_vec(&old).unwrap()).unwrap();
    let read = read_receipt(&dir, &agent(), HASH, linux()).unwrap();
    assert_eq!(read.files, Vec::<String>::new());
    assert_eq!(read.node_stamp, None);
    assert!(!read.node_unchanged(), "no stamp: the start cannot tell");
}

#[test]
fn a_codex_path_moved_aside_is_not_a_missing_file() {
    let data = TestDir::new("receipt-codex-path");
    let dir = folder(&data, HASH, None);
    let entry = AgentEntry::new(agent(), "Codex", "codex-acp", "bin/codex-acp.js");
    let vendor = "node_modules/@openai/codex/vendor/x86_64-pc-windows-msvc";
    for file in [
        "node_modules/codex-acp/bin/codex-acp.js".to_owned(),
        "node_modules/@openai/codex/package.json".to_owned(),
        // Smart App Control is On: the folder is moved aside, its `rg.exe` with it.
        format!("{vendor}/codex-path.sac-off/rg.exe"),
    ] {
        let path = file
            .split('/')
            .fold(dir.clone(), |path, part| path.join(part));
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_atomic(&path, b"x").unwrap();
    }
    let lock = serde_json::json!({ "packages": {
        "node_modules/codex-acp": { "dependencies": { "@openai/codex": "1.0.0" } },
        "node_modules/@openai/codex": {},
    }});
    let keys = key_paths(&dir, &entry, &lock.to_string());
    assert_eq!(
        keys,
        vec![
            "node_modules/codex-acp/bin/codex-acp.js",
            "node_modules/codex-acp",
            "node_modules/@openai/codex",
        ]
    );
    // Even a receipt that lists the folder (or a file in it) reads complete without it.
    let mut written = receipt(HASH);
    written.files = keys;
    written.files.push(format!("{vendor}/codex-path"));
    written.files.push(format!("{vendor}/codex-path/rg.exe"));
    assert_eq!(missing_file(&dir, &entry, &written), None);
}
