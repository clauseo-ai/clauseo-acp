//! The embedded snapshot's data: G16's pins, the sizes measured for them, Node 24.22.0's
//! archives and the agents' hosts.

use clauseo_protocol::AgentId;

use super::{
    AgentEntry, AgentSizes, NPM_REGISTRY, NodeArchive, NodeEntry, Snapshot, SnapshotError,
};
use crate::platform::{Arch, Os};

/// G16's pins file, the one home of the adapters' versions.
pub(super) const PINS_PACKAGE_JSON: &str = include_str!("../../../limits/tests/live/package.json");
/// G16's lock for it.
pub(super) const PINS_LOCK: &str = include_str!("../../../limits/tests/live/package-lock.json");

/// The `short_hash` of [`PINS_LOCK`]'s bytes when [`SIZES`] was measured. A test fails when the
/// lock changes and this does not: measure the sizes again, then update both. Only that test
/// reads it.
#[cfg(test)]
pub(super) const SIZES_STAMP: &str = "0963c2b034cfbf7a";

/// Download and unpacked bytes of one agent's packages for one platform, measured from the
/// registry for the lock [`SIZES_STAMP`] names (Linux rows are glibc's; musl uses them too).
const SIZES: [(&str, Os, Arch, u64, u64); 12] = [
    (
        "claude-acp",
        Os::Darwin,
        Arch::Arm64,
        112_272_099,
        284_447_720,
    ),
    (
        "claude-acp",
        Os::Darwin,
        Arch::X64,
        116_398_379,
        292_936_176,
    ),
    ("claude-acp", Os::Win32, Arch::X64, 124_933_588, 304_272_912),
    (
        "claude-acp",
        Os::Win32,
        Arch::Arm64,
        120_819_899,
        291_797_016,
    ),
    ("claude-acp", Os::Linux, Arch::X64, 122_081_905, 300_872_245),
    (
        "claude-acp",
        Os::Linux,
        Arch::Arm64,
        122_340_028,
        300_225_917,
    ),
    (
        "codex-acp",
        Os::Darwin,
        Arch::Arm64,
        138_852_372,
        353_173_843,
    ),
    ("codex-acp", Os::Darwin, Arch::X64, 148_075_606, 377_267_611),
    ("codex-acp", Os::Win32, Arch::X64, 167_303_482, 472_371_597),
    (
        "codex-acp",
        Os::Win32,
        Arch::Arm64,
        156_863_952,
        412_140_965,
    ),
    ("codex-acp", Os::Linux, Arch::X64, 167_703_509, 467_510_455),
    (
        "codex-acp",
        Os::Linux,
        Arch::Arm64,
        160_119_520,
        423_188_897,
    ),
];

/// Node's version.
const NODE_VERSION: &str = "24.22.0";
/// Node's download folder for it.
const NODE_BASE: &str = "https://nodejs.org/dist/v24.22.0/";
/// Each archive: platform, file name, size, unpacked size, SHA-256 from `SHASUMS256.txt` (its
/// signature checked with the Node release keys: Richard Lau, `C82F A3AE 1CBE DC6B E46B 9360
/// C43C EC45 C17A B93C`).
const NODE_ARCHIVES: [(Os, Arch, &str, u64, u64, &str); 6] = [
    (
        Os::Darwin,
        Arch::Arm64,
        "node-v24.22.0-darwin-arm64.tar.gz",
        53_049_576,
        197_365_224,
        "6a9724d9b50a5f22b4816cded441b5a8d9ddae546191a5e5b7efa5a5e2ee26d5",
    ),
    (
        Os::Darwin,
        Arch::X64,
        "node-v24.22.0-darwin-x64.tar.gz",
        54_356_645,
        200_534_581,
        "495e3f273c645f3c4677d438304cdfc701729ea5dec5728f5c8e76a2dece4b91",
    ),
    (
        Os::Win32,
        Arch::X64,
        "node-v24.22.0-win-x64.zip",
        37_733_703,
        107_312_622,
        "f9683a19877e12a9b167305c8db71040d812876359de21f0d2e97c3f321f80c3",
    ),
    (
        Os::Win32,
        Arch::Arm64,
        "node-v24.22.0-win-arm64.zip",
        33_770_411,
        95_553_518,
        "3a5c209a5bc59e65b3bc0fa13ed5e166a26bd3d55332847f498b76b037408d61",
    ),
    (
        Os::Linux,
        Arch::X64,
        "node-v24.22.0-linux-x64.tar.gz",
        58_256_361,
        201_678_212,
        "28ea0a438de9a13bd7891fd758eee6c7819ee8af5442dfd8d45bd65e56d4228c",
    ),
    (
        Os::Linux,
        Arch::Arm64,
        "node-v24.22.0-linux-arm64.tar.gz",
        57_997_377,
        198_130_272,
        "b7e7991d5407815adfa74a2a9cd4db683552c8ce32ff4a295c22e972849c3f5d",
    ),
];

/// Hosts every install contacts.
const INSTALL_HOSTS: [&str; 2] = ["nodejs.org", "registry.npmjs.org"];
// The sign-in hosts below are S18's: G7 row 1 confirms them against a real sign-in.
/// Claude's sign-in hosts. `claude.com` is where Claude Code's authorize page now lives
/// (`https://claude.com/cai/oauth/authorize`, the address the brief's fake opens).
const CLAUDE_SIGNIN_HOSTS: [&str; 4] = [
    "claude.ai",
    "claude.com",
    "console.anthropic.com",
    "platform.claude.com",
];
/// Codex's sign-in hosts.
const CODEX_SIGNIN_HOSTS: [&str; 2] = ["auth.openai.com", "chatgpt.com"];

/// The embedded snapshot, or why it did not build.
pub(super) fn build_embedded() -> Result<Snapshot, SnapshotError> {
    let agents = [
        (
            "claude-acp",
            "Claude",
            "@agentclientprotocol/claude-agent-acp",
            &CLAUDE_SIGNIN_HOSTS[..],
        ),
        (
            "codex-acp",
            "Codex",
            "@agentclientprotocol/codex-acp",
            &CODEX_SIGNIN_HOSTS[..],
        ),
    ];
    let mut entries = Vec::with_capacity(agents.len());
    for (id, name, root, signin) in agents {
        let agent_id = AgentId::new(id).map_err(|_bad| SnapshotError::ReservedId(id.to_owned()))?;
        let mut entry = AgentEntry::new(agent_id, name, root, "dist/index.js");
        entry.install_hosts = INSTALL_HOSTS.iter().map(|&host| host.to_owned()).collect();
        entry.signin_hosts = signin.iter().map(|&host| host.to_owned()).collect();
        entry.sizes = SIZES
            .iter()
            .filter(|row| row.0 == id)
            .map(
                |&(_, os, arch, download_bytes, unpacked_bytes)| AgentSizes {
                    os,
                    arch,
                    download_bytes,
                    unpacked_bytes,
                },
            )
            .collect();
        entries.push(entry);
    }
    Snapshot::new(
        PINS_PACKAGE_JSON,
        PINS_LOCK,
        NPM_REGISTRY,
        embedded_node(),
        entries,
    )
}

/// Node 24.22.0's entry.
pub(super) fn embedded_node() -> NodeEntry {
    let archives = NODE_ARCHIVES
        .iter()
        .filter_map(|&(os, arch, name, size, unpacked, sha)| {
            NodeArchive::new(os, arch, name, size, unpacked, sha)
        })
        .collect();
    NodeEntry::new(NODE_VERSION, NODE_BASE, archives)
}
