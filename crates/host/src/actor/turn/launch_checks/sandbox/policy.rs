//! The policy of one jailed agent launch (R17 PR-D, decision record 0051), built on the seam's
//! thread from the launch's prep, what the actor read for it ([`Grants`]) and its reservation's
//! ports. Every path comes from the helpers that made it (`RunFolders`, `state_folder`,
//! `launch_checks::paths`), never derived again here.
//!
//! - **Write:** the matter folder (as the agent runs in it, `agent::session_folder`), the
//!   session's `run/<session>/tmp` (its `TMPDIR`), the agent's state folder for the matter
//!   (Codex's `CODEX_HOME`, Claude's `CLAUDE_CONFIG_DIR`; the session's `work` folder for an
//!   agent without a kind), and the matter's notes folder, as the data fence grants it. Clauseo's
//!   own config file in the state folder (Codex's `config.toml`, Claude's `settings.json`) is
//!   read-only.
//! - **Login** ([`login_grant`], the one place it is chosen): Codex's `auth.json` link in its
//!   state folder (`credential_link`) only for Clauseo's own login home's file, never while the
//!   login is reused or the file is the lawyer's own by identity (a jailed launch of a reused
//!   Codex login is refused before the jail, `sign_in::check_launch`); Claude's store in
//!   Clauseo's login home (`claude_store`: its store files only), never the lawyer's own store;
//!   no Keychain anywhere.
//! - **Read and run**, in this order (on Windows' tier B the runner makes the agent's `PATH` of
//!   exactly this list, #792): the folder of the agent's program (Node for an adapter, links
//!   followed), its install folder from its install record, stetdoc's folder (`path_prepend`),
//!   then with the browser its sidecars' folders and Chrome's. A runtime folder in the lawyer's
//!   Documents, Desktop or Downloads ([`personal_folders`]: on Windows the known folders,
//!   wherever `OneDrive` moved them), in the lawyer's `~/.claude` or `~/.codex` or their custom
//!   `CLAUDE_CONFIG_DIR` or `CODEX_HOME`, in `~/.ssh`, `~/.aws`, `~/.gnupg` or
//!   `~/.config/gcloud` (`off_limits`), or in any
//!   matter of the registry refuses the launch. Read paths never go on `PATH`, which is these
//!   folders then the OS's (`jailed_env::path`).
//! - **Read only in the matter:** its files over the checkpoint size cap (`over_cap.rs`,
//!   decision record 0038), besides the sandbox's own read-only names.
//! - **Read only:** the CA bundle (`ca.rs`), the profile's skills folders, and what exists of the
//!   session's skill packs, its relay token and agent-browser's config.
//! - **Hidden:** the data folder (the parts above are allowed again after it) and every other
//!   matter of the registry, open or not. Another matter inside this one refuses the launch
//!   ([`nested_matter`]): the sandbox cannot hide a folder inside the matter.
//! - **Network:** only the session proxy's listeners, the relay and the reservation's ports
//!   (`proxy_ports`); with the browser, its loopback ports and socket folder.
//!
//! Allowed folders (R18) are none yet: D2b adds them.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, ErrorCode, HostError, MatterPath, ThreadId};
use clauseo_sandbox::{Loopback, Policy, PolicyBuilder};

use super::super::super::super::Actor;
use super::super::relay::CaseLaw;
use super::super::{Prepared, RunFolders};
use crate::agent::state_folder::StateLink;
use crate::agent::{codex, jailed_env};
use crate::config::sandbox::CaRoots;
use crate::error::Error;

/// Whether the jail denies loading native code (and on Windows running programs) from the agent's
/// writable roots. The product's choice alone, pinned per release: never a setting, a variable or
/// anything an agent reaches, and once a release turns it on nothing turns it off. Off until
/// stage D's pinned Python runtime ships (lxml and pandas load native code from the matter's
/// virtual environments).
///
/// TODO(stage C's native-code PR): `Policy` has no such field on this base; once it lands this
/// constant goes into [`policy`] through its builder.
pub(crate) const NATIVE_CODE_DENY: bool = false;

/// The lawyer's folders no runtime folder may sit in (`home/<name>`).
const PERSONAL_FOLDERS: [&str; 3] = ["Documents", "Desktop", "Downloads"];

/// Where the lawyer's Documents, Desktop and Downloads are for a home ([`personal_folders`]).
pub(crate) type PersonalFolders =
    std::sync::Arc<dyn Fn(Option<&Path>) -> Vec<PathBuf> + Send + Sync>;

/// What a jailed launch's policy takes besides its prep, read on the actor when the seam's job
/// starts: paths and flags only, no file system call.
#[derive(Clone)]
pub(crate) struct Grants {
    /// The data folder: hidden whole, the session's own parts allowed again.
    pub(crate) data: PathBuf,
    /// The agent's kind; `None` for one without.
    pub(crate) kind: Option<AgentKind>,
    /// The agent's program, absolute with a folder.
    pub(crate) command: PathBuf,
    /// The agent's install folder from its install record (`Sandbox::with_install_folders`).
    pub(crate) install: Option<PathBuf>,
    /// stetdoc's folder, put first on the agent's `PATH` (`HostConfig::with_agent_path_prepend`).
    pub(crate) stetdoc: Option<PathBuf>,
    /// Every other matter's folder in the registry, as stored, open or not.
    pub(crate) others: Vec<PathBuf>,
    /// The profile's skills folders for the agent (each write's plugin folder, else its root).
    pub(crate) skills: Vec<PathBuf>,
    /// The browser entry's sidecars (`clauseo-mcp`, agent-browser).
    pub(crate) sidecars: Vec<PathBuf>,
    /// Whether desktop says the agent's login is reused (the lawyer's own store).
    pub(crate) reused: bool,
    /// The agent's `network.ca_certs` file.
    pub(crate) ca_cert: Option<PathBuf>,
    /// The OS roots' source (`Sandbox::with_ca_roots`); `None`: `rustls-native-certs`'.
    pub(crate) roots: Option<CaRoots>,
    /// The lawyer's home folder: the policy's, and the base of [`PERSONAL_FOLDERS`].
    pub(crate) home: Option<PathBuf>,
    /// The lawyer's custom Claude Code and Codex homes ([`lawyers_agent_homes`]): no runtime
    /// folder may sit in one.
    pub(crate) agent_homes: Vec<PathBuf>,
    /// The lawyer's Documents, Desktop and Downloads for a home, looked up off the actor:
    /// [`personal_folders`] (tests pass their own).
    pub(crate) personal: PersonalFolders,
    /// The checkpoint walk's size cap: a matter file larger than it is read-only (`over_cap.rs`).
    pub(crate) file_cap: u64,
    /// Clauseo's own login home for the agent (`Locations::agent_home`): the only home whose
    /// `auth.json` link the jail gets.
    pub(crate) login_home: PathBuf,
}

/// A jailed launch's policy and the variables its agent gets with it: the proxy's (they carry its
/// token), the CA bundle's and `jailed_env::host_vars` (`PATH` from the policy's runtime
/// folders), `None` to remove.
pub(crate) struct Jail {
    pub(crate) policy: Policy,
    pub(crate) env: Vec<(OsString, Option<OsString>)>,
}

/// The launch's jail: its runtime folders ([`runtime`]), its CA bundle written into the run
/// folder (`ca.rs`), its [`policy`] with `reserved` (the reservation's ports) and the agent's
/// variables. Refused first when another matter sits inside this one ([`nested_matter`]). File
/// system calls: off the actor.
pub(crate) fn jail(prepared: &Prepared, grants: &Grants, reserved: &[u16]) -> Result<Jail, Error> {
    nested_matter(grants, prepared)?;
    let runtime = runtime(grants, prepared)?;
    let ca = super::ca::write(
        &prepared.run.root,
        grants.roots.as_ref(),
        grants.ca_cert.as_deref(),
    );
    let listener = prepared
        .run
        .egress
        .as_ref()
        .and_then(|egress| egress.agent_listener());
    let mut ports = reserved.to_vec();
    ports.extend(listener.as_ref().map(|listener| listener.port));
    let policy = policy(prepared, grants, runtime, ca.as_deref(), &ports)
        .map_err(|error| super::refused(&error))?;
    let mut env = listener.map(|listener| listener.env).unwrap_or_default();
    env.extend(jailed_env::host_vars(policy.runtime()));
    if let Some(ca) = &ca {
        env.extend(
            super::ca::CA_VARS
                .iter()
                .map(|name| (OsString::from(name), Some(ca.as_os_str().to_owned()))),
        );
    }
    Ok(Jail { policy, env })
}

/// What `thread`'s jailed launch's policy takes, read on the actor (`policy::Grants`): paths
/// and flags only. Refuses a Codex agent while it would start in its ask mode, before anything
/// is reserved or asked of the seam: a jailed Codex starts only in the jailed preset
/// (`codex::JAILED_MODE`, R17 PR-D2c).
pub(super) fn grants(actor: &Actor, thread: ThreadId) -> Result<Grants, Error> {
    let row = &actor.check_thread(thread)?.row;
    let agent = &row.agent;
    let kind = AgentKind::of(agent);
    if kind == Some(AgentKind::Codex) && codex::launch_mode(true) != codex::JAILED_MODE {
        return Err(Error::refused(
            ErrorCode::AgentLaunchFailed,
            "The sandbox couldn't start Codex: it would run in its ask mode, which the sandbox \
             does not take yet.",
        ));
    }
    let sandbox = actor.protection.sandbox();
    let matters = actor.matters.iter().chain(&actor.forgotten);
    let others = matters
        .filter(|(id, _)| **id != row.matter)
        .map(|(_, entry)| PathBuf::from(&entry.row.folder_path))
        .collect();
    let skills = actor
        .profile
        .launch_writes(agent)
        .iter()
        .map(|write| write.plugin().unwrap_or_else(|| write.root()).to_path_buf())
        .collect();
    Ok(Grants {
        data: actor.effective.locations().data_dir().to_owned(),
        kind,
        command: command(actor, thread)?,
        install: sandbox
            .and_then(|sandbox| sandbox.install_folders.as_ref())
            .and_then(|folders| folders(agent)),
        stetdoc: actor.wiring.path_prepend.clone(),
        others,
        skills,
        sidecars: super::super::browser::sidecars(actor, agent).unwrap_or_default(),
        reused: actor.wiring.sign_in.reused.contains(agent),
        ca_cert: actor
            .effective
            .settings()
            .network
            .ca_certs
            .get(agent.as_str())
            .cloned(),
        roots: sandbox.and_then(|sandbox| sandbox.ca_roots.clone()),
        home: actor.home.clone(),
        agent_homes: lawyers_agent_homes(actor),
        personal: std::sync::Arc::new(personal_folders),
        file_cap: clauseo_matters::FILE_CAP_BYTES,
        login_home: actor.effective.locations().agent_home(agent.as_str()),
    })
}

/// The lawyer's own `CLAUDE_CONFIG_DIR` and `CODEX_HOME` from their environment (the inherited
/// one, as a launch reads it), absolute ones only. Read on the actor, no file system call.
fn lawyers_agent_homes(actor: &Actor) -> Vec<PathBuf> {
    let names = [crate::agent::home::CLAUDE_CONFIG_VAR, codex::HOME_VAR];
    let values: Vec<OsString> = match &actor.wiring.inherited_env {
        Some(env) => env
            .iter()
            .filter(|(name, _)| names.iter().any(|want| name.as_os_str() == *want))
            .map(|(_, value)| value.clone())
            .collect(),
        None => names.iter().filter_map(std::env::var_os).collect(),
    };
    values
        .into_iter()
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .collect()
}

/// The agent's program (`command`), which must be an absolute path with a folder.
fn command(actor: &Actor, thread: ThreadId) -> Result<PathBuf, Error> {
    let row = &actor.check_thread(thread)?.row;
    let settings = actor.effective.agent(row.agent.as_str()).ok_or_else(|| {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("no agent {} in the settings", row.agent),
        )
    })?;
    let command = Path::new(&settings.command);
    command
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .map(|_| command.to_path_buf())
        .ok_or_else(|| {
            Error::refused(
                ErrorCode::AgentLaunchFailed,
                "The sandbox couldn't start the agent: its program has no folder.",
            )
        })
}

/// The agent's install folder: the folder of its program with every link followed, so a
/// `/opt/homebrew/bin/claude` link grants the real install and not all of `bin`. File system
/// calls: off the actor.
pub(super) fn install_folder(command: &Path) -> Result<PathBuf, Error> {
    dunce::canonicalize(command)
        .ok()
        .and_then(|real| real.parent().map(Path::to_path_buf))
        .ok_or_else(|| {
            Error::refused(
                ErrorCode::AgentLaunchFailed,
                "The sandbox couldn't start the agent: its program wasn't found.",
            )
        })
}

/// The folders the agent reads and runs, in `PATH` order: its program's (Node for an adapter,
/// links followed), its install record's, stetdoc's, then with the browser its sidecars' and
/// Chrome's; each once. Refuses the launch when the program's or the install's is missing, or
/// one sits in the lawyer's Documents, Desktop or Downloads ([`Grants::personal`]), in their
/// Claude Code or Codex home or a credential store (`off_limits`), or in a matter of the
/// registry (this one or another: the sandbox would let the agent read there again); the
/// message names no path. File system calls: off the actor.
pub(crate) fn runtime(grants: &Grants, prepared: &Prepared) -> Result<Vec<PathBuf>, Error> {
    let run = &prepared.run;
    let mut runtime = vec![install_folder(&grants.command)?];
    if let Some(folder) = &grants.install {
        runtime.push(dunce::canonicalize(folder).map_err(|_missing| {
            launch_failed("The sandbox couldn't start the agent: its install folder wasn't found.")
        })?);
    }
    runtime.extend(
        grants
            .stetdoc
            .as_deref()
            .and_then(|folder| dunce::canonicalize(folder).ok()),
    );
    if run.browser_session.is_some() {
        runtime.extend(grants.sidecars.iter().filter_map(|sidecar| {
            let real = dunce::canonicalize(sidecar).ok()?;
            real.parent().map(Path::to_path_buf)
        }));
        runtime.extend(run.chrome.as_deref().and_then(chrome_folder));
    }
    off_limits::refuse(grants, prepared, &runtime, off_limits::FOLD_CASE)?;
    let mut seen = std::collections::HashSet::new();
    runtime.retain(|folder| seen.insert(folder.clone()));
    Ok(runtime)
}

/// The lawyer's Documents, Desktop and Downloads for `home`: `home/<name>` for each, and on
/// Windows the known folders too (`SHGetKnownFolderPath` through `clauseo_settings::SystemDirs`,
/// unchecked), which follow a redirection such as `OneDrive`'s Known Folder Move that the text
/// path misses. Paths as given: [`runtime`] resolves them. File system calls on Windows
/// (Documents' bounded check): off the actor.
pub(crate) fn personal_folders(home: Option<&Path>) -> Vec<PathBuf> {
    let folders = home
        .iter()
        .flat_map(|home| PERSONAL_FOLDERS.iter().map(move |name| home.join(name)));
    #[cfg(windows)]
    let folders = {
        use clauseo_settings::{KnownFolder, PlatformDirs as _, SystemDirs};
        let known = [
            KnownFolder::Documents,
            KnownFolder::Desktop,
            KnownFolder::Downloads,
        ];
        folders.chain(
            known
                .into_iter()
                .filter_map(|folder| SystemDirs.known_folder(folder)),
        )
    };
    folders.collect()
}

/// Refuses the launch with `matter.folder_nested` when another matter of the registry
/// (`Grants::others`: forgotten ones too) sits inside this one: the policy cannot hide a folder
/// inside the matter it lets the agent write, so the jail could not keep the two apart. The
/// error's `files` names the inner folder's path in this matter (`matter_path_in`: compared
/// canonical, as stored where the folder is gone), the first in sorted order; the message and
/// the log name no path, the log the kind only.
pub(crate) fn nested_matter(grants: &Grants, prepared: &Prepared) -> Result<(), Error> {
    let mut nested: Vec<MatterPath> = grants
        .others
        .iter()
        .filter_map(|other| clauseo_matters::matter_path_in(&prepared.stored, other))
        .collect();
    nested.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
    let Some(first) = nested.into_iter().next() else {
        return Ok(());
    };
    tracing::warn!(
        kind = "a matter inside this matter",
        "the sandbox refused the agent's launch"
    );
    let refusal = HostError::new(
        ErrorCode::MatterFolderNested,
        "Another matter's folder is inside this matter's folder, so the Safe workspace can't keep \
         them apart.",
    );
    Err(Error::Refused(refusal.with_files(vec![first])))
}

/// The folder Chrome runs from: on macOS the `.app` bundle that holds it, elsewhere its own
/// folder; links followed.
fn chrome_folder(chrome: &Path) -> Option<PathBuf> {
    let real = dunce::canonicalize(chrome).ok()?;
    let bundle = real.ancestors().find(|folder| {
        folder
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    });
    bundle.or_else(|| real.parent()).map(Path::to_path_buf)
}

/// The policy itself (see the module header), `runtime` from [`runtime`], `ca` the bundle and
/// `ports` the reservation's and the proxy's agent listener. `build` walks the matter: off the
/// actor.
pub(crate) fn policy(
    prepared: &Prepared,
    grants: &Grants,
    runtime: Vec<PathBuf>,
    ca: Option<&Path>,
    ports: &[u16],
) -> Result<Policy, clauseo_sandbox::Error> {
    let run = &prepared.run;
    let matter = crate::agent::session_folder(&prepared.stored);
    let state = run.state.clone().unwrap_or_else(|| run.work.clone());
    let mut builder = Policy::builder(&matter, &state)
        .tmpdir(run.tmp())
        .notes(&run.notes);
    if let Some(home) = &grants.home {
        builder = builder.home(home);
    }
    if let (Some(kind), Some(state)) = (grants.kind, &run.state) {
        builder = builder.read_only_in_state(state.join(clauseo_config(kind)));
    }
    builder = login_grant(builder, grants, prepared);
    for folder in runtime {
        builder = builder.runtime(folder);
    }
    for path in read_only(grants, run, ca) {
        builder = builder.read(path);
    }
    builder = builder.over_cap(super::over_cap::over_cap(&matter, grants.file_cap));
    builder = builder.hidden(&grants.data);
    for other in &grants.others {
        // A matter inside this one refuses the launch (`nested_matter`); one holding it is left
        // to the default deny, as hiding it would refuse the build.
        if !other.starts_with(&prepared.stored) && !prepared.stored.starts_with(other) {
            builder = builder.hidden(other);
        }
    }
    builder = network(builder, run, ports);
    builder.build()
}

/// Clauseo's own config file in `kind`'s state folder, written whole before each launch
/// (`state_folder::prepare`): read-only in the jail, as the data fence denies it today.
fn clauseo_config(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Codex => crate::agent::codex::HOME_CONFIG,
        AgentKind::Claude => crate::agent::home::CLAUDE_SETTINGS_FILE,
    }
}

/// How the agent's login reaches its jail, chosen here alone so the choice can switch (stage C
/// checks Claude Code's file fallback under a Keychain denial on macOS): a Codex agent's
/// `auth.json` link in its state folder (`credential_link`, only that name), when it is the
/// file of Clauseo's own login home (`Grants::login_home`), the login is not reused and the
/// file is not the lawyer's own by identity (`CodexPrepared::lawyers_login`; ruling (a): such
/// a launch is refused before this, so this is the second fence); a Claude agent's store in
/// Clauseo's login home (`claude_store`: only the store's files and lock folders), never the
/// lawyer's own store while the login is reused. No
/// Keychain rule in any policy (`keychain` stays off, ruling of 2026-11-08). Clauseo never reads
/// the store.
pub(crate) fn login_grant(
    builder: PolicyBuilder,
    grants: &Grants,
    prepared: &Prepared,
) -> PolicyBuilder {
    match grants.kind {
        Some(AgentKind::Codex) => {
            let linked = prepared.codex.as_ref().filter(|codex| {
                !grants.reused
                    && !codex.lawyers_login
                    && codex.home == grants.login_home
                    && matches!(codex.link, Some(Ok(StateLink::Same | StateLink::Linked)))
            });
            match linked {
                Some(codex) => builder
                    .credential_link(codex.state.join(crate::agent::state_folder::LOGIN_FILE)),
                None => builder,
            }
        }
        Some(AgentKind::Claude) => match prepared.claude_home.as_deref() {
            Some(home) if !grants.reused => builder.claude_store(home),
            _ => builder,
        },
        None => builder,
    }
}

/// What the agent reads and never writes: the CA bundle, the profile's skills folders, and what
/// exists of the session's skill packs, relay token and agent-browser's config.
fn read_only(grants: &Grants, run: &RunFolders, ca: Option<&Path>) -> Vec<PathBuf> {
    let mut read: Vec<PathBuf> = ca.map(Path::to_path_buf).into_iter().collect();
    read.extend(
        grants
            .skills
            .iter()
            .filter(|folder| folder.is_dir())
            .cloned(),
    );
    if run.packs_written.is_some() && run.skill_packs.is_dir() {
        read.push(run.skill_packs.clone());
    }
    if matches!(run.case_law, CaseLaw::Open(_)) && run.relay_token.is_file() {
        read.push(run.relay_token.clone());
    }
    if run.browser_session.is_some() {
        let config = grants
            .data
            .join(crate::browser::CONFIG_DIR)
            .join(crate::browser::CONFIG_FILE);
        if config.is_file() {
            read.push(config);
        }
    }
    read
}

/// The network: `ports` (the reservation's and the proxy's agent listener), the browser listener
/// and the relay's; with the browser, its loopback ports and (macOS, Linux) socket folder.
fn network(mut builder: PolicyBuilder, run: &RunFolders, ports: &[u16]) -> PolicyBuilder {
    let browser_port = run.egress.as_ref().and_then(|egress| egress.port());
    let relay = match &run.case_law {
        CaseLaw::Open(relay) => Some(relay.port()),
        CaseLaw::Absent | CaseLaw::Unavailable(_) | CaseLaw::Ended(..) => None,
    };
    for port in ports.iter().copied().chain(browser_port).chain(relay) {
        builder = builder.proxy_port(port);
    }
    if let Some(session) = &run.browser_session {
        builder = builder.loopback(Loopback::Ports(session.ports.loopback()));
        #[cfg(unix)]
        {
            builder = builder.browser_socket_dir(&session.socket_dir);
        }
    }
    builder
}

/// A refused launch with `message` (`agent.launch_failed`), never a path.
fn launch_failed(message: &str) -> Error {
    Error::refused(ErrorCode::AgentLaunchFailed, message)
}

mod off_limits;

#[cfg(test)]
mod tests;
