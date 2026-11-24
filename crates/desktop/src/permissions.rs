//! The first-start permissions step's two commands: `permissions` lists what this build on this
//! computer needs, each with its state, and `ask_permission` asks for one. Only what is needed
//! is listed, so a row the lawyer sees is one that matters:
//!
//! - macOS `files_documents`, when the matters folder is inside Documents, which macOS guards
//!   (TCC). Its answer cannot be read silently before Clauseo asks (a read either works, or
//!   shows the dialog, or fails with no way to tell a refusal from no answer), so the row says
//!   `not_asked` until Clauseo asked on this build, then what macOS answered then
//!   (`store.rs`). Asking lists Documents from a child process: macOS shows its dialog naming
//!   Clauseo, and the grant covers every process Clauseo starts.
//! - macOS `keychain`, when reading one of Clauseo's own keychain items would make macOS ask
//!   (an item a build signed otherwise wrote): found out silently (`permissions/mac.rs`).
//!   Asking reads each such item with the dialog allowed. Account names only, never a value.
//! - Windows `controlled_folder_access`, when Defender's Controlled folder access is on and
//!   the matters folder is under a protected folder. What it blocks is the assistants' own tools
//!   (the Windows VM's block, event 1123, named `powershell.exe`), not `Clauseo.exe`, so
//!   whether Clauseo itself is allowed changes nothing. Only an administrator can allow a tool,
//!   so asking grants nothing: the row says to move the matters folder or ask IT. Its setting
//!   is read in this process from the registry (`permissions/windows.rs`), once per process
//!   ([`ReadOnce`]): no child process, so no console and nothing to outlive the app.
//!
//! Linux lists none. The OS reads sit behind [`Probe`], so the rules are tested anywhere.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use clauseo_protocol::{ErrorCode, HostError};
use clauseo_settings::SystemFiles;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime, State};

use crate::open::AppData;
use store::{Answer, Asked, Store};
use system::SystemProbe;

#[cfg(target_os = "macos")]
mod mac;
mod store;
mod system;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

/// A permission the step may list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum PermissionId {
    /// macOS: Clauseo's access to the Documents folder, where the matters folder is.
    FilesDocuments,
    /// macOS: reading Clauseo's own stored keys without a keychain dialog.
    Keychain,
    /// Windows: Defender's Controlled folder access, which blocks writes under Documents.
    ControlledFolderAccess,
}

/// Where a permission stands. Not `PermissionState`: protocol's (an agent's permission request)
/// already has that name in the TypeScript bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum OsPermissionState {
    /// Clauseo has not asked on this build; the OS's answer cannot be read before it does.
    NotAsked,
    /// Granted when Clauseo asked on this build.
    Granted,
    /// Refused or cancelled when Clauseo asked on this build.
    Denied,
    /// The OS would show its dialog at the next read.
    WouldAsk,
    /// It blocks Clauseo, and only an administrator can change that.
    Blocking,
    /// Could not tell: a silent check failed in a way that says neither.
    Unknown,
}

/// One row of the step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct PermissionRow {
    /// Which permission.
    pub(crate) id: PermissionId,
    /// Where it stands.
    pub(crate) state: OsPermissionState,
    /// The folder it is about (`files_documents`: Documents).
    pub(crate) folder: Option<String>,
    /// Where in the OS's settings the lawyer changes it, when Clauseo cannot.
    pub(crate) settings_path: Option<String>,
    /// More words: the keychain items concerned (service and account), or what else helps.
    pub(crate) note: Option<String>,
}

/// Where macOS lets the lawyer change Clauseo's access to Documents.
pub(crate) const FILES_SETTINGS: &str =
    "System Settings › Privacy & Security › Files and Folders › Clauseo";
/// Where Windows lets an administrator allow Clauseo through Controlled folder access.
pub(crate) const CFA_SETTINGS: &str = "Windows Security › Virus & threat protection › \
                                       Ransomware protection › Allow an app through Controlled \
                                       folder access";
/// What the lawyer can do about Controlled folder access: the block falls on the assistants'
/// tools, which Clauseo cannot allow, so the way out is another folder or IT.
pub(crate) const CFA_NOTE: &str = "Windows blocks the assistants' tools, such as PowerShell, from \
                                   saving in protected folders. Choose a matters folder outside \
                                   Documents, Desktop and Pictures, or ask your IT team.";

/// The OS a listing follows: a test plays any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Mac,
    Windows,
    Other,
}

/// One of Clauseo's keychain items: a generic password, by service and account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeychainItem {
    pub(crate) service: String,
    pub(crate) account: String,
}

/// What the silent keychain check found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KeychainCheck {
    /// Items whose read would show macOS's dialog.
    pub(crate) would_ask: Vec<KeychainItem>,
    /// Items whose silent read failed in a way that says neither.
    pub(crate) unknown: Vec<KeychainItem>,
}

/// Defender's Controlled folder access, as the registry gives it (`windows.rs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Cfa {
    /// `EnableControlledFolderAccess` is 1 (on, blocking); 2 to 4 only audit or guard disks.
    pub(crate) on: bool,
    /// `ControlledFolderAccessProtectedFolders`: those beside Windows' defaults.
    pub(crate) protected: Vec<PathBuf>,
}

/// The OS reads behind a listing and an ask. Every method may block.
pub(crate) trait Probe: Send + Sync {
    /// The OS the rules follow.
    fn platform(&self) -> Platform;
    /// Where new matters go: the settings' matters folder, else the default one.
    fn matters_folder(&self) -> Option<PathBuf>;
    /// macOS: the user's Documents folder.
    fn documents(&self) -> Option<PathBuf>;
    /// macOS: the silent read of Clauseo's keychain items.
    fn keychain(&self) -> KeychainCheck;
    /// Windows: Controlled folder access, `None` when it cannot be read.
    fn controlled_folder_access(&self) -> Option<Cfa>;
    /// Windows: the folders Controlled folder access protects by default.
    fn protected_defaults(&self) -> Vec<PathBuf>;
    /// macOS: lists `folder` from a child process, as long as it takes; whether that worked.
    fn list_folder(&self, folder: &Path) -> Result<bool, HostError>;
    /// macOS: reads `item` with the dialog allowed; whether it read (or has gone).
    fn read_keychain(&self, item: &KeychainItem) -> bool;
}

/// Lists the permissions this build needs on this computer, each with its state: macOS's
/// Documents and keychain, Windows' Controlled folder access, each only when it matters. Granted
/// to `main` and to `settings` (`capabilities/settings.json`): Settings shows the same block for a
/// lawyer who already has matters and never sees the first screen.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn permissions<R: Runtime>(
    app: AppHandle<R>,
    app_data: State<'_, AppData>,
) -> Result<Vec<PermissionRow>, HostError> {
    crate::ipc::timed("permissions", async {
        let store = system_store(&app, &app_data.0);
        blocking(move || Ok(list(&SYSTEM, &store.asked()).rows)).await
    })
    .await
}

/// Asks for `id`, off the UI thread and as long as the OS's dialog stays up, and answers its
/// row after. `command.invalid` for one `permissions` would not list now. Granted to `main` and
/// `settings` alike: the OS prompt rises only when the lawyer presses Allow there.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn ask_permission<R: Runtime>(
    app: AppHandle<R>,
    app_data: State<'_, AppData>,
    id: PermissionId,
) -> Result<PermissionRow, HostError> {
    crate::ipc::timed("ask_permission", async {
        let store = system_store(&app, &app_data.0);
        blocking(move || ask(&SYSTEM, &store, id)).await
    })
    .await
}

/// The OS, its Controlled folder access read once per process.
static SYSTEM: ReadOnce<SystemProbe> = ReadOnce::new(SystemProbe);

/// The store in `data_dir` for this app's build.
fn system_store<R: Runtime>(app: &AppHandle<R>, data_dir: &Path) -> Store {
    let build = store::build_id(
        &app.package_info().version.to_string(),
        option_env!("CLAUSEO_VERSION_SUFFIX"),
        store::exe_identity(),
    );
    Store::new(Arc::new(SystemFiles), data_dir, build)
}

/// Runs `work` on a blocking thread: the OS reads wait on dialogs, processes and Defender.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, HostError> + Send + 'static,
) -> Result<T, HostError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| {
            HostError::new(
                ErrorCode::Internal,
                format!("the permissions read did not finish: {error}"),
            )
        })?
}

/// The rows, and what an ask needs from the reads behind them.
pub(crate) struct Listing {
    pub(crate) rows: Vec<PermissionRow>,
    /// The Documents folder, when `files_documents` is listed.
    documents: Option<PathBuf>,
    /// The keychain items an ask reads.
    keychain: Vec<KeychainItem>,
}

/// What `probe` says this build needs, with the answers `asked` recorded on it.
pub(crate) fn list(probe: &dyn Probe, asked: &Asked) -> Listing {
    let mut listing = Listing {
        rows: Vec::new(),
        documents: None,
        keychain: Vec::new(),
    };
    match probe.platform() {
        Platform::Mac => {
            if let Some(documents) = documents_inside(probe) {
                let state = asked
                    .get(&PermissionId::FilesDocuments)
                    .map_or(OsPermissionState::NotAsked, |answer| answer.state());
                listing.rows.push(documents_row(&documents, state));
                listing.documents = Some(documents);
            }
            let check = probe.keychain();
            if let Some(row) = keychain_row(&check, asked.get(&PermissionId::Keychain).copied()) {
                listing.rows.push(row);
            }
            listing.keychain = check.would_ask.into_iter().chain(check.unknown).collect();
        }
        Platform::Windows => {
            if cfa_blocks(probe) {
                listing.rows.push(cfa_row());
            }
        }
        Platform::Other => {}
    }
    listing
}

/// Asks for `id` through `probe` and records the answer in `store`; its row after.
pub(crate) fn ask(
    probe: &dyn Probe,
    store: &Store,
    id: PermissionId,
) -> Result<PermissionRow, HostError> {
    let listing = list(probe, &store.asked());
    if !listing.rows.iter().any(|row| row.id == id) {
        return Err(HostError::new(
            ErrorCode::CommandInvalid,
            format!("{id:?} is not needed on this computer"),
        ));
    }
    match id {
        PermissionId::FilesDocuments => {
            let Some(documents) = listing.documents else {
                return Err(HostError::new(
                    ErrorCode::CommandInvalid,
                    "there is no Documents folder to ask for",
                ));
            };
            let answer = Answer::from_granted(probe.list_folder(&documents)?);
            tracing::info!(?answer, "permissions: asked for Documents");
            keep(store, id, answer);
            Ok(documents_row(&documents, answer.state()))
        }
        PermissionId::Keychain => {
            // One by one: macOS shows one dialog per item.
            let refused: Vec<KeychainItem> = listing
                .keychain
                .into_iter()
                .filter(|item| !probe.read_keychain(item))
                .collect();
            let answer = Answer::from_granted(refused.is_empty());
            tracing::info!(
                ?answer,
                refused = refused.len(),
                "permissions: asked the keychain"
            );
            keep(store, id, answer);
            Ok(PermissionRow {
                id,
                state: answer.state(),
                folder: None,
                settings_path: None,
                note: items_note(&refused),
            })
        }
        PermissionId::ControlledFolderAccess => Ok(cfa_row()),
    }
}

/// Records `answer`; a write that fails is logged, and the row still says what macOS answered.
fn keep(store: &Store, id: PermissionId, answer: Answer) {
    if let Err(error) = store.record(id, answer) {
        tracing::warn!(kind = ?error.kind(), ?id, "permissions.json was not written");
    }
}

/// The Documents folder, canonical, when the matters folder is inside it.
fn documents_inside(probe: &dyn Probe) -> Option<PathBuf> {
    let documents = canonical(&probe.documents()?);
    let matters = canonical(&probe.matters_folder()?);
    is_inside(&matters, &documents).then_some(documents)
}

/// `files_documents`' row for `documents` in `state`.
fn documents_row(documents: &Path, state: OsPermissionState) -> PermissionRow {
    PermissionRow {
        id: PermissionId::FilesDocuments,
        state,
        folder: Some(documents.display().to_string()),
        settings_path: (state == OsPermissionState::Denied).then(|| FILES_SETTINGS.to_owned()),
        note: None,
    }
}

/// `keychain`'s row: listed when an item would ask or could not be told, or when Clauseo
/// asked on this build (`recorded`), so a granted row stays shown and is not asked again.
fn keychain_row(check: &KeychainCheck, recorded: Option<Answer>) -> Option<PermissionRow> {
    let state = match recorded {
        Some(answer) => answer.state(),
        None if !check.would_ask.is_empty() => OsPermissionState::WouldAsk,
        None if !check.unknown.is_empty() => OsPermissionState::Unknown,
        None => return None,
    };
    let items: Vec<KeychainItem> = check
        .would_ask
        .iter()
        .chain(&check.unknown)
        .cloned()
        .collect();
    Some(PermissionRow {
        id: PermissionId::Keychain,
        state,
        folder: None,
        settings_path: None,
        note: items_note(&items),
    })
}

/// The items by service and account, never a value; `None` for none.
fn items_note(items: &[KeychainItem]) -> Option<String> {
    (!items.is_empty()).then(|| {
        items
            .iter()
            .map(|item| format!("{} ({})", item.service, item.account))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// Whether Controlled folder access blocks writes to the matters folder: it reads, it is on,
/// and the matters folder is under a protected folder. Its allowed apps are not consulted: the
/// writes it blocks are the assistants' tools' (`powershell.exe` on the VM), never
/// `Clauseo.exe`'s, so Clauseo being allowed does not lift the block.
fn cfa_blocks(probe: &dyn Probe) -> bool {
    let Some(cfa) = probe.controlled_folder_access() else {
        return false;
    };
    let Some(matters) = probe.matters_folder() else {
        return false;
    };
    if !cfa.on {
        return false;
    }
    let matters = canonical(&matters);
    probe
        .protected_defaults()
        .into_iter()
        .chain(cfa.protected)
        .any(|folder| is_inside(&matters, &canonical(&folder)))
}

/// `controlled_folder_access`' row: always blocking, since what is blocked is the assistants'
/// tools (such as `powershell.exe`), which only an administrator can allow; the settings path
/// is for IT, the note tells the lawyer to move the matters folder or ask IT.
fn cfa_row() -> PermissionRow {
    PermissionRow {
        id: PermissionId::ControlledFolderAccess,
        state: OsPermissionState::Blocking,
        folder: None,
        settings_path: Some(CFA_SETTINGS.to_owned()),
        note: Some(CFA_NOTE.to_owned()),
    }
}

/// `path` with its longest existing start resolved (links, `\\?\`-free on Windows), the rest
/// kept as written: a matters folder not made yet still compares.
pub(crate) fn canonical(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut base = path;
    loop {
        if let Ok(mut real) = dunce::canonicalize(base) {
            real.extend(rest.iter().rev());
            return real;
        }
        match (base.parent(), base.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                base = parent;
            }
            _ => return path.to_owned(),
        }
    }
}

/// Whether `child` is `parent` or inside it, by component, ignoring case as macOS's and
/// Windows' usual file systems do.
pub(crate) fn is_inside(child: &Path, parent: &Path) -> bool {
    let mut child = child.components();
    parent
        .components()
        .all(|part| child.next().is_some_and(|other| same_part(part, other)))
}

fn same_part(a: std::path::Component<'_>, b: std::path::Component<'_>) -> bool {
    a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
}

/// One registry place Defender's Controlled folder access setting may be kept in
/// (`windows.rs`), as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CfaLayer {
    /// The key or its value could not be read (access denied, or another error).
    Unreadable,
    /// Readable, with no `EnableControlledFolderAccess` here.
    Unset,
    /// `EnableControlledFolderAccess` here.
    Set(u32),
}

/// Controlled folder access from its registry places, most specific first (group policy, MDM,
/// Defender's own): the first that sets `EnableControlledFolderAccess` decides; 1 is on
/// (blocking), 0 off and 2 to 4 only audit or guard disks, and anything else cannot be told.
/// Places that set nothing fall through, and none set reads as off, Defender's default; when no
/// place sets it and one could not be read, it cannot be told (`None`, as no layer at all).
/// `protected`: the folders the places add to Windows' defaults.
pub(crate) fn cfa_from_layers(layers: &[CfaLayer], protected: Vec<PathBuf>) -> Option<Cfa> {
    let mut unreadable = layers.is_empty();
    for layer in layers {
        match layer {
            CfaLayer::Unreadable => unreadable = true,
            CfaLayer::Unset => {}
            CfaLayer::Set(enable) => {
                return (*enable <= 4).then(|| Cfa {
                    on: *enable == 1,
                    protected,
                });
            }
        }
    }
    (!unreadable).then(|| Cfa {
        on: false,
        protected,
    })
}

/// A probe whose Controlled folder access is read at most once per value: the first read is
/// kept, and a read already under way is waited for, not started again. A page reload, a second
/// `permissions` or an `ask_permission` reuses it; a change to Defender's setting shows at the
/// next start. The other reads pass through.
pub(crate) struct ReadOnce<P> {
    probe: P,
    cfa: OnceLock<Option<Cfa>>,
}

impl<P> ReadOnce<P> {
    pub(crate) const fn new(probe: P) -> Self {
        Self {
            probe,
            cfa: OnceLock::new(),
        }
    }
}

impl<P: Probe> Probe for ReadOnce<P> {
    fn platform(&self) -> Platform {
        self.probe.platform()
    }
    fn matters_folder(&self) -> Option<PathBuf> {
        self.probe.matters_folder()
    }
    fn documents(&self) -> Option<PathBuf> {
        self.probe.documents()
    }
    fn keychain(&self) -> KeychainCheck {
        self.probe.keychain()
    }
    fn controlled_folder_access(&self) -> Option<Cfa> {
        self.cfa
            .get_or_init(|| self.probe.controlled_folder_access())
            .clone()
    }
    fn protected_defaults(&self) -> Vec<PathBuf> {
        self.probe.protected_defaults()
    }
    fn list_folder(&self, folder: &Path) -> Result<bool, HostError> {
        self.probe.list_folder(folder)
    }
    fn read_keychain(&self, item: &KeychainItem) -> bool {
        self.probe.read_keychain(item)
    }
}
