//! The policy's paths, each with the parameter name the profile reads it by.

use std::path::{Path, PathBuf};

use crate::{ClaudeStore, PathRule, Policy};

/// The policy's paths with their parameter names.
pub(super) struct Params {
    pub(super) runtime: Vec<(String, PathBuf)>,
    pub(super) read: Vec<(String, PathBuf)>,
    pub(super) allowed: Vec<(String, PathBuf)>,
    pub(super) notes: Option<PathBuf>,
    pub(super) tmpdir: Option<PathBuf>,
    pub(super) credential_link: Option<PathBuf>,
    pub(super) claude_files: Vec<(String, PathBuf)>,
    pub(super) claude_locks: Vec<(String, PathBuf)>,
    pub(super) read_only: Vec<(String, PathRule)>,
    pub(super) link_targets: Vec<(String, PathBuf)>,
    pub(super) hidden: Vec<(String, PathBuf)>,
    pub(super) read_only_state: Vec<(String, PathBuf)>,
    pub(super) keychains: Option<(String, PathBuf)>,
}

impl Params {
    pub(super) fn new(policy: &Policy) -> Params {
        let store = policy.claude_store();
        Params {
            runtime: numbered("RUNTIME", policy.runtime()),
            read: numbered("READ", policy.read()),
            allowed: numbered("ALLOWED", policy.allowed_folders()),
            notes: policy.notes().map(Path::to_owned),
            tmpdir: policy.session_tmpdir().map(Path::to_owned),
            credential_link: policy.credential_link().map(Path::to_owned),
            claude_files: numbered(
                "CLAUDE_FILE",
                &store.map(ClaudeStore::files).unwrap_or_default(),
            ),
            claude_locks: numbered(
                "CLAUDE_LOCK",
                &store.map(ClaudeStore::lock_folders).unwrap_or_default(),
            ),
            read_only: policy
                .read_only_in_matter()
                .iter()
                .enumerate()
                .map(|(i, rule)| (format!("READ_ONLY_{i}"), rule.clone()))
                .collect(),
            // Only targets an allow rule covers; see `Policy::link_targets`.
            link_targets: numbered("LINK_TARGET", policy.link_targets()),
            hidden: numbered("HIDDEN", policy.hidden()),
            read_only_state: numbered("STATE_READ_ONLY", policy.read_only_in_state()),
            keychains: policy
                .keychain()
                .then(|| policy.home())
                .flatten()
                .map(|home| {
                    (
                        "KEYCHAINS".to_owned(),
                        home.join("Library").join("Keychains"),
                    )
                }),
        }
    }
}

fn numbered(prefix: &str, paths: &[PathBuf]) -> Vec<(String, PathBuf)> {
    paths
        .iter()
        .enumerate()
        .map(|(i, path)| (format!("{prefix}_{i}"), path.clone()))
        .collect()
}
