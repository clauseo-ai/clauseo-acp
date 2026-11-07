//! The launch message built from a policy ([`LaunchMessage::from_policy`]), the paths a policy
//! grants ([`granted_paths`]), and M3's check before any entry is applied
//! ([`codex_reuse_in_policy`]).

use std::path::{Path, PathBuf};

use super::paths::{self, WinPath, codex_reuse};
use super::{LAUNCH_VERSION, LaunchError, LaunchMessage};

/// [`LaunchMessage::from_policy`]'s work.
pub(super) fn message_from_policy(
    policy: &crate::Policy,
    pool: &crate::windows::pool::PoolUser,
    spec: &clauseo_process::LaunchSpec,
    ca_bundle: Option<&Path>,
) -> Result<LaunchMessage, LaunchError> {
    let path = |field: &'static str, path: &Path| {
        path.to_str().map(str::to_owned).ok_or(LaunchError::Field {
            field,
            rule: "is not UTF-8",
        })
    };
    let text = |field: &'static str, value: &std::ffi::OsStr| {
        value.to_str().map(str::to_owned).ok_or(LaunchError::Field {
            field,
            rule: "is not UTF-8",
        })
    };
    let runtime = policy
        .runtime()
        .iter()
        .map(|p| path("runtime", p))
        .collect::<Result<Vec<_>, _>>()?;
    let granted = granted_paths(policy)
        .into_iter()
        .map(|p| path("granted", p))
        .collect::<Result<Vec<_>, _>>()?;
    let message = LaunchMessage {
        version: LAUNCH_VERSION,
        pool_user: pool.sid.clone(),
        pool_name: pool.name.clone(),
        matter_sid: None,
        program: path("program", spec.program())?,
        args: spec
            .args()
            .iter()
            .map(|arg| text("args", arg))
            .collect::<Result<_, _>>()?,
        cwd: path("cwd", spec.cwd())?,
        env: spec
            .env()
            .iter()
            .map(|(name, value)| Ok((text("env", name)?, text("env", value)?)))
            .collect::<Result<_, LaunchError>>()?,
        state: path("state", policy.state())?,
        tmpdir: policy
            .session_tmpdir()
            .map(|p| path("tmpdir", p))
            .transpose()?,
        runtime,
        granted,
        ca_bundle: ca_bundle.map(|p| path("ca_bundle", p)).transpose()?,
        credential_link: policy
            .credential_link()
            .map(|p| path("credential_link", p))
            .transpose()?,
    };
    message.check()?;
    Ok(message)
}

/// Every path `policy` lets the agent reach: the matter, state, notes and session temp folders,
/// the runtime and read paths, the allowed folders, the Claude store's folder and the credential
/// link.
pub fn granted_paths(policy: &crate::Policy) -> Vec<&Path> {
    let mut paths = vec![policy.matter(), policy.state()];
    paths.extend(policy.notes());
    paths.extend(policy.session_tmpdir());
    paths.extend(policy.runtime().iter().map(PathBuf::as_path));
    paths.extend(policy.read().iter().map(PathBuf::as_path));
    paths.extend(policy.allowed_folders().iter().map(PathBuf::as_path));
    paths.extend(policy.claude_store().map(crate::ClaudeStore::dir));
    paths.extend(policy.credential_link());
    paths
}

/// M3 before any entry is applied (for the launcher, PR6 and R17): refuses a policy any of whose
/// granted paths lies in the lawyer's `<profile>\.codex`. The launcher must also refuse a
/// credential link that is that folder's `auth.json` by file ID, as the outer runner does.
pub fn codex_reuse_in_policy(
    policy: &crate::Policy,
    profile: &WinPath,
) -> Result<(), paths::CodexReuse> {
    let texts: Vec<String> = granted_paths(policy)
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    codex_reuse(profile, texts.iter().map(|t| ("granted", t.as_str())), &[])
}
