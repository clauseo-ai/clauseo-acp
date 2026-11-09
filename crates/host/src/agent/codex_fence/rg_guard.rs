//! Linux: the Codex fence's ripgrep guard (R17 PR-D1; decision record 0051, the residual of
//! R17 PR-C's fence). Bubblewrap expands the fence's deny globs ([`super::STATE_GLOBS`]) by
//! running `rg`, looked up by name on Codex's own `PATH` (codex-rs rust-v0.162.1
//! `linux-sandbox/src/bwrap.rs:901`); without one, Codex's fallback walker follows links and
//! masks their targets, so a planted link could hide a host file from other matters' sandboxes.
//! And an `rg` the agent can write would run with Codex's own rights at every sandboxed command.
//!
//! So before a Codex launch [`check`] resolves the first executable `rg` in Codex's order: its
//! `arg0` folder first (made fresh by Codex at its start in its state folder's `tmp`, holding only
//! Codex's own aliases, never an `rg`, so not searched), then the vendored
//! `vendor/<triple>/codex-path` of the `@openai/codex` package the adapter loads, its own nested
//! `node_modules` first ([`vendor_folders`]; Codex puts it first on its `PATH`,
//! `arg0/src/lib.rs:160-171`), then the launch's `PATH` (the settings', with `path_prepend`
//! first, as `agent::launch_spec` merges it). The launch is refused with
//! `agent.launch_failed` when there is none, or when its canonical path is inside a folder the
//! session writes or Codex keeps (the matter, the run folder, the notes, the state folder). The
//! refusal and the log name no path.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;

use crate::error::Error;

/// The vendored folder's name under `vendor/<triple>` that Codex puts first on its `PATH`.
const CODEX_PATH: &str = "codex-path";

/// What the guard reads, taken on the actor with no file system call.
#[derive(Debug, Clone, Default)]
pub(crate) struct Inputs {
    /// The `PATH` the launch spec gives Codex (`agent::launch_spec`), when it has one.
    pub(crate) path: Option<OsString>,
    /// The adapter's script (`args[0]` in the settings), whose `node_modules` hold Codex.
    pub(crate) script: Option<PathBuf>,
}

impl Inputs {
    /// The guard's inputs for a Codex launch of `settings`: the `PATH` `agent::launch_spec`
    /// gives it from the `inherited` environment, the settings' and the profile hook's
    /// variables (`hook`) and `path_prepend` (the host forces no `PATH`), and the adapter's
    /// script. No file system call: on the actor.
    pub(crate) fn of(
        settings: &clauseo_settings::AgentLaunch,
        hook: std::collections::BTreeMap<OsString, OsString>,
        inherited: impl Iterator<Item = (OsString, OsString)>,
        path_prepend: Option<&Path>,
    ) -> Self {
        let spec = crate::agent::launch_spec(
            settings,
            hook,
            std::collections::BTreeMap::new(),
            &crate::agent::codex::DROPPED_VARS,
            inherited,
            PathBuf::from("/"),
            path_prepend,
        );
        let path = spec.ok().and_then(|spec| {
            spec.env()
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
                .map(|(_, value)| value.clone())
        });
        Self {
            path,
            script: settings.args.first().map(PathBuf::from),
        }
    }
}

/// The target triple of the vendored Codex for this machine (`@openai/codex`'s Linux builds).
fn triple() -> String {
    format!("{}-unknown-linux-musl", std::env::consts::ARCH)
}

/// The vendored `codex-path` folders next to the adapter's `script`, nearest first, in Node's
/// lookup order for the adapter's `@openai/codex`: for each folder above the script (one that is
/// not itself a `node_modules`), its `node_modules`; so the adapter's own nested
/// `<adapter>/node_modules` comes before the install's `<install>/node_modules`. In each, every
/// `@openai/codex*` package's `vendor/<triple>/codex-path` that is a folder, by name.
pub(crate) fn vendor_folders(script: &Path) -> Vec<PathBuf> {
    let triple = triple();
    let mut found = Vec::new();
    let folders = script
        .ancestors()
        .skip(1)
        .filter(|folder| folder.file_name().is_none_or(|name| name != "node_modules"));
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(folder.join("node_modules").join("@openai")) else {
            continue;
        };
        let mut packages: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("codex"))
            .map(|entry| entry.path().join("vendor").join(&triple).join(CODEX_PATH))
            .filter(|folder| folder.is_dir())
            .collect();
        packages.sort();
        found.extend(packages);
    }
    found
}

/// The first executable regular file named `rg` in `folders`, as a `PATH` lookup finds it
/// (links followed); a relative folder resolves against `cwd`.
pub(crate) fn find_rg(folders: &[PathBuf], cwd: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    folders.iter().find_map(|folder| {
        let candidate = cwd.join(folder).join("rg");
        let meta = std::fs::metadata(&candidate).ok()?;
        (meta.is_file() && meta.permissions().mode() & 0o111 != 0).then_some(candidate)
    })
}

/// The guard: refuses a Codex launch whose first `rg` ([`find_rg`] over the vendored folders,
/// then `inputs.path`, a relative entry resolved in `cwd`, the folder the agent runs in) is
/// missing or lies, canonically, inside one of `writable` (each compared canonical when it
/// resolves, as given too). File system calls: off the actor.
pub(crate) fn check(inputs: &Inputs, cwd: &Path, writable: &[&Path]) -> Result<(), Error> {
    let mut folders = inputs
        .script
        .as_deref()
        .map(vendor_folders)
        .unwrap_or_default();
    if let Some(path) = &inputs.path {
        // Linux `PATH` entries are `:`-separated bytes (as `std::env::split_paths` reads them).
        use std::os::unix::ffi::OsStrExt as _;
        folders.extend(
            path.as_bytes()
                .split(|byte| *byte == b':')
                .filter(|part| !part.is_empty())
                .map(|part| PathBuf::from(std::ffi::OsStr::from_bytes(part))),
        );
    }
    let Some(rg) = find_rg(&folders, cwd) else {
        tracing::warn!("Codex has no ripgrep on its path; the launch is refused");
        return Err(Error::refused(
            ErrorCode::AgentLaunchFailed,
            "Clauseo can't start Codex safely: its search tool (ripgrep) is missing. Reinstall \
             Codex in Clauseo's settings.",
        ));
    };
    let real = dunce::canonicalize(&rg).unwrap_or(rg);
    let inside = writable.iter().any(|root| {
        real.starts_with(root)
            || dunce::canonicalize(root).is_ok_and(|canonical| real.starts_with(canonical))
    });
    if inside {
        tracing::warn!("Codex's ripgrep is in a folder the agent can write; the launch is refused");
        return Err(Error::refused(
            ErrorCode::AgentLaunchFailed,
            "Clauseo can't start Codex safely: the search tool (ripgrep) it would run sits in a \
             folder the agent can change.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "rg_guard_tests.rs"]
mod tests;
