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
