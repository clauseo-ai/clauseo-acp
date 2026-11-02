//! The policy's paths, each with the parameter name the profile reads it by.

use std::path::{Path, PathBuf};

use crate::{ClaudeStore, PathRule, Policy};

/// The policy's paths with their parameter names.
pub(super) struct Params {
    pub(super) runtime: Vec<(String, PathBuf)>,
    pub(super) read: Vec<(String, PathBuf)>,
    pub(super) allowed: Vec<(String, PathBuf)>,
    pub(super) notes: Option<PathBuf>,
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
        let allowed = allowed_paths(policy);
        Params {
            runtime: numbered("RUNTIME", policy.runtime()),
            read: numbered("READ", policy.read()),
            allowed: numbered("ALLOWED", policy.allowed_folders()),
            notes: policy.notes().map(Path::to_owned),
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
            // A target that is or holds an allowed path gets no rule: a subpath rule on it
            // would deny the matter (or the root) itself, and the default deny already refuses
            // it, its ancestors' metadata aside.
            link_targets: policy
                .links()
                .iter()
                .enumerate()
                .filter_map(|(i, f)| f.target.as_ref().map(|t| (i, t)))
                .filter(|(_, target)| !allowed.iter().any(|a| a.starts_with(target)))
                .map(|(i, target)| (format!("LINK_TARGET_{i}"), target.clone()))
                .collect(),
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

/// Every path the policy allows, for the link-target check.
fn allowed_paths(policy: &Policy) -> Vec<PathBuf> {
    let mut all = vec![policy.matter().to_owned(), policy.state().to_owned()];
    all.extend(policy.runtime().iter().cloned());
    all.extend(policy.read().iter().cloned());
    all.extend(policy.allowed_folders().iter().cloned());
    all.extend(policy.notes().map(Path::to_owned));
    all.extend(policy.credential_link().map(Path::to_owned));
    if let Some(store) = policy.claude_store() {
        all.extend(store.files());
        all.extend(store.lock_folders());
    }
    all
}

fn numbered(prefix: &str, paths: &[PathBuf]) -> Vec<(String, PathBuf)> {
    paths
        .iter()
        .enumerate()
        .map(|(i, path)| (format!("{prefix}_{i}"), path.clone()))
        .collect()
}
