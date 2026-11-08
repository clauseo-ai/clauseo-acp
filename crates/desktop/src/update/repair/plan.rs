//! Whether this app may run its own installer again, and which one: the release's tag from the
//! build's release mode, the per-user installer of the running version and architecture, its
//! `.sig`, and the key that must have signed it.

use clauseo_host::InstallKind;
use clauseo_protocol::UpdateState;
use url::Url;

use super::super::file_binding::installer_arch;
use super::super::plugin::is_plain_version;
use crate::commands::ReleaseMode;

/// Where every release's assets are downloaded from: stage F's release, one tag per version,
/// never `/releases/latest` (the installer must be the running version's).
pub(super) const RELEASES: &str = "https://github.com/clauseo-ai/clauseo-acp/releases/download";

/// The release's tag for `version` in `mode`: `v<version>` for a release build, `test-v<version>`
/// for a test build (`release.yml`'s two tag forms), none for a dry run, which publishes nothing.
pub(super) fn tag(mode: ReleaseMode, version: &str) -> Option<String> {
    match mode {
        ReleaseMode::Release => Some(format!("v{version}")),
        ReleaseMode::Test => Some(format!("test-v{version}")),
        ReleaseMode::Dry => None,
    }
}

/// The per-user installer of `version` for a Rust `target_arch`, as the release names and signs
/// it (`file_binding::update_file_name`'s Windows name): `Clauseo_<version>_<x64|arm64>-setup.exe`.
pub(super) fn installer_name(target_arch: &str, version: &str) -> Option<String> {
    installer_arch(target_arch).map(|arch| format!("Clauseo_{version}_{arch}-setup.exe"))
}

/// What decides whether the repair may run.
#[derive(Debug, Clone, Copy)]
pub(super) struct Inputs<'a> {
    /// `std::env::consts::OS`.
    pub(super) os: &'a str,
    /// `std::env::consts::ARCH`: the running build's, so a repair never moves architecture.
    pub(super) target_arch: &'a str,
    /// How this copy was installed (`install_kind::detect`).
    pub(super) kind: InstallKind,
    /// The build's release mode, compiled in (`commands::build_release_mode`).
    pub(super) mode: ReleaseMode,
    /// The running version (`tauri.conf.json`'s).
    pub(super) version: &'a str,
    /// The updater key for `mode` ([`super::key_for`]); none when the build has none.
    pub(super) pubkey: Option<&'a str>,
    /// The updater's state, when it was read: an install on its way refuses.
    pub(super) update: Option<&'a UpdateState>,
}

/// One repair's download: what to fetch, and what its signature must name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Plan {
    /// The running version, which the signature's `version:` must name.
    pub(super) version: String,
    /// The installer's name, which the signature's `file:` must name.
    pub(super) file: String,
    /// The installer's URL.
    pub(super) installer: Url,
    /// Its signature's URL: the installer's, with `.sig`.
    pub(super) signature: Url,
    /// The updater key the signature must verify under.
    pub(super) pubkey: String,
}

/// Why the repair may not run here, as a fixed line for host.log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NotEligible(pub(super) &'static str);

/// Whether the updater's `state` refuses a repair: an update scheduled or installing, or updates
/// managed by someone else. Read again just before the hand-over (`flow.rs`).
pub(super) fn update_refuses(state: &UpdateState) -> bool {
    matches!(
        state,
        UpdateState::Installing { .. }
            | UpdateState::Scheduled { .. }
            | UpdateState::Managed { .. }
    )
}

/// The plan for `inputs`, or why there is none: Windows only, a per-user install only (per
/// machine and the rest are the firm's), a release or test build with its key, a plain dotted
/// version, a known architecture, no update scheduled or installing, and updates not managed
/// (a firm policy that turns them off).
pub(super) fn plan(inputs: Inputs<'_>) -> Result<Plan, NotEligible> {
    if inputs.os != "windows" {
        return Err(NotEligible("not windows"));
    }
    if inputs.kind != InstallKind::CurrentUser {
        return Err(NotEligible("not a per-user install"));
    }
    match inputs.update {
        Some(UpdateState::Installing { .. } | UpdateState::Scheduled { .. }) => {
            return Err(NotEligible("an update is installing"));
        }
        // The firm's policy turned updates off (or someone else updates this copy): the
        // installer is the firm's to run, never this app's.
        Some(UpdateState::Managed { .. }) => return Err(NotEligible("updates are managed")),
        _ => {}
    }
    if !is_plain_version(inputs.version) {
        return Err(NotEligible("the version is not plain dotted numbers"));
    }
    let tag = tag(inputs.mode, inputs.version).ok_or(NotEligible("a dry-run build"))?;
    let pubkey = inputs
        .pubkey
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or(NotEligible("no updater key"))?;
    let file = installer_name(inputs.target_arch, inputs.version)
        .ok_or(NotEligible("no installer for this architecture"))?;
    let installer = Url::parse(&format!("{RELEASES}/{tag}/{file}"))
        .map_err(|_unparsed| NotEligible("the installer's URL does not parse"))?;
    let signature = Url::parse(&format!("{RELEASES}/{tag}/{file}.sig"))
        .map_err(|_unparsed| NotEligible("the signature's URL does not parse"))?;
    Ok(Plan {
        version: inputs.version.to_owned(),
        file,
        installer,
        signature,
        pubkey: pubkey.to_owned(),
    })
}

/// Whether the admin copy is missing, from what was read: whether the record names one (an
/// error when it can't be read, which the launch check treats as missing too), and whether a
/// plain file is at its place in Program Files. The repair runs only then.
pub(super) fn copy_missing(recorded: Result<bool, ()>, present: bool) -> bool {
    recorded != Ok(true) || !present
}

/// Whether a sealed attempt's folder holds its installer and nothing else: `names` as the folder
/// lists them, `.` and `..` aside, must be exactly `installer` (the `.sig` stays in memory).
/// Any other name, a stream's name (`file:stream`), a second entry or none refuses.
pub(super) fn only_installer(names: &[String], installer: &str) -> bool {
    let mut rest = names.iter().filter(|name| *name != "." && *name != "..");
    rest.next().is_some_and(|name| name == installer) && rest.next().is_none()
}
