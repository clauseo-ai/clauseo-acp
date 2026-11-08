//! The host's `Updater` over `tauri-plugin-updater` (decision record 0037).
//!
//! - [`Source`]: where the plugin looks and which key it trusts. `UpdaterBuilder` is used, never
//!   the plugin's `updater()`, so an endpoint and a key set in code (the `e2e` build's test
//!   release) apply to that updater alone.
//! - `check` and `download` run on Tauri's runtime ([`on_runtime`]): the host's pool gets a
//!   future over a oneshot, and dropping it aborts the task, which cancels the request.
//! - `download` is the plugin's `Update::download`, which verifies the signature and the signed
//!   version before it returns the bytes. They are saved as the one file of
//!   `updates/<version>/`, written beside the folder first and moved in whole
//!   ([`save_download`]), so the folder never holds a half-written file.
//! - [`install`] rebuilds the update from its `raw_json` (`restore_update`, which always requires
//!   a signed version) and hands the plugin's `install` the file's bytes, which it verifies again.
//! - [`Target`] (`arch.rs`): an x64 per-user install emulated on ARM64 checks for the arm64
//!   installer, and again for its own when the release has none ([`check_for`]). The target of
//!   the check that found the update rides with it in [`Pending`], and the install's updater
//!   and both file bindings take it from there.
//! - Between them, before the download is saved and again before its bytes go to `install`, the
//!   file binding (`file_binding.rs`, [`Source::pubkey`]) refuses a file whose verified
//!   signature names another file than this platform's installer, or another version.
//!
//! An error reaches the host as a fixed line by its kind ([`failure`]), never the plugin's text,
//! which can hold the endpoint's URL or a path (0047).

use std::fs;
use std::future::Future;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_host::{Downloaded, InstallKind, Offer, Opaque, ProgressSink, Updater, UpdaterError};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{Error as PluginError, Update, UpdaterExt as _};
use url::Url;

use super::arch::{self, Target};
use super::cached;
use super::file_binding;

/// How long a request may take to connect, and to go without a byte, before it fails as a
/// network error: the host waits on a check or a download with no deadline of its own.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// The name of a saved download in its version's folder. The plugin's `install` takes bytes, so
/// the name only tells a person what it is.
const INSTALLER: &str = if cfg!(windows) {
    "Clauseo-setup.exe"
} else if cfg!(target_os = "macos") {
    "Clauseo.app.tar.gz"
} else {
    "Clauseo-update"
};

/// Where the plugin looks for `latest.json`, and the key it checks signatures with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Source {
    /// `tauri.conf.json`'s `plugins.updater`: its endpoints and its pubkey.
    Config,
    /// One endpoint and a key set in code: the `e2e` build's test release, and the tests'.
    Endpoint {
        /// The `latest.json` to read.
        url: Url,
        /// The minisign public key, as `tauri signer` writes it (base64).
        pubkey: String,
    },
}

impl Source {
    /// The plugin's updater over this source, looking up `target`'s key, with the request
    /// timeouts. Fails with `EmptyEndpoints` for [`Source::Config`] while the config names none
    /// (`choice.rs` never asks then). The check and the install both build theirs here.
    pub(super) fn updater<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        target: Target,
    ) -> Result<tauri_plugin_updater::Updater, PluginError> {
        let builder = app.updater_builder().configure_client(client);
        let builder = match target.plugin_target() {
            Some(key) => builder.target(key),
            None => builder,
        };
        let builder = match self {
            Source::Config => builder,
            Source::Endpoint { url, pubkey } => {
                builder.endpoints(vec![url.clone()])?.pubkey(pubkey.clone())
            }
        };
        builder.build()
    }

    /// The key this source's updater checks signatures with, for the file binding:
    /// `config`'s `plugins.updater.pubkey` (the plugin's own), or the key set in code. None
    /// while the config sets none, so the binding refuses.
    pub(super) fn pubkey(&self, config: &tauri::Config) -> Option<String> {
        match self {
            Source::Config => config
                .plugins
                .0
                .get("updater")
                .and_then(|updater| updater.get("pubkey"))
                .and_then(serde_json::Value::as_str)
                .filter(|key| !key.trim().is_empty())
                .map(str::to_owned),
            Source::Endpoint { pubkey, .. } => Some(pubkey.clone()),
        }
    }
}

/// The settings every request of the updater gets (`configure_client`, which the plugin applies
/// to the check's client and the download's): the timeouts, and HTTPS only, so neither an
/// `http://` URL nor a redirect from HTTPS to HTTP is ever followed. The plugin refuses an
/// `http://` endpoint in a release build but only warns for a download URL, and checks neither
/// after a redirect. Every build, `e2e` included: its test release is on GitHub, over HTTPS,
/// and no test serves an update over HTTP.
pub(super) fn client(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    builder
        .https_only(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
}

/// What an [`Offer`]'s handle holds, and so the [`Downloaded`] made from it: the plugin's update,
/// the source it came from, and the target its check asked for, to rebuild it at the install.
pub(super) struct Pending {
    /// The update `check` returned.
    pub(super) update: Update,
    /// Where it came from.
    pub(super) source: Source,
    /// The key the check that found it looked up: never one a failed check asked for.
    pub(super) target: Target,
}

/// Whether this app moves to the arm64 build, for its install kind (`arch::probe`; the tests'
/// own).
pub(super) type Arm64Probe = fn(InstallKind) -> bool;

/// The plugin, adapted to the host's [`Updater`].
pub(crate) struct PluginUpdater<R: Runtime> {
    app: AppHandle<R>,
    source: Source,
    /// Found once, at start (`install_kind.rs`).
    kind: InstallKind,
    /// `Locations::updates_dir()`.
    updates_dir: PathBuf,
    /// Asked before each check, off the async workers.
    arm64: Arm64Probe,
}

impl<R: Runtime> PluginUpdater<R> {
    /// The plugin over `source`, for an app installed as `kind`, saving downloads under
    /// `updates_dir`.
    pub(crate) fn new(
        app: AppHandle<R>,
        source: Source,
        kind: InstallKind,
        updates_dir: PathBuf,
    ) -> Self {
        Self {
            app,
            source,
            kind,
            updates_dir,
            arm64: arch::probe,
        }
    }

    /// The same, deciding the arm64 move with `probe` instead of this machine's.
    #[cfg(test)]
    pub(super) fn with_arm64_probe(self, probe: Arm64Probe) -> Self {
        Self {
            arm64: probe,
            ..self
        }
    }

    /// The target this adapter's check asks for first.
    #[cfg(test)]
    pub(super) fn wanted(&self) -> Target {
        wanted(self.arm64, self.kind)
    }
}

impl<R: Runtime> Updater for PluginUpdater<R> {
    fn install_kind(&self) -> InstallKind {
        self.kind
    }

    /// While the repair runs the installer itself (`repair.rs`), no update install begins:
    /// `auto`'s cached install at start included, which no `InstallUpdate` asks for.
    fn install_held(&self) -> bool {
        super::repair::running()
    }

    fn check(&self) -> BoxFuture<'static, Result<Option<Offer>, UpdaterError>> {
        let app = self.app.clone();
        let source = self.source.clone();
        let (probe, kind) = (self.arm64, self.kind);
        let pubkey = self.source.pubkey(self.app.config());
        let updates = self.updates_dir.clone();
        on_runtime(async move {
            let wanted = tauri::async_runtime::spawn_blocking(move || wanted(probe, kind))
                .await
                .unwrap_or(Target::Native);
            let offer = check_for(source, wanted, |source, target| {
                source.updater(&app, target)
            })
            .await?;
            let Some(offer) = offer else {
                return Ok(None);
            };
            // Last run's download is reused only when it binds to this offer (`cached.rs`).
            tauri::async_runtime::spawn_blocking(move || {
                cached::checked(offer, &updates, pubkey.as_deref())
            })
            .await
            .map(Some)
            .map_err(|_stopped| UpdaterError::Other("the cached file's check stopped".to_owned()))
        })
    }

    fn download(
        &self,
        offer: Offer,
        mut progress: ProgressSink,
    ) -> BoxFuture<'static, Result<Downloaded, UpdaterError>> {
        let Some(pending) = offer.handle.downcast_ref::<Pending>() else {
            return Box::pin(futures::future::ready(Err(UpdaterError::Other(
                "the offer did not come from this updater".to_owned(),
            ))));
        };
        let update = pending.update.clone();
        let target = pending.target;
        let pubkey = pending.source.pubkey(self.app.config());
        let updates_dir = self.updates_dir.clone();
        on_runtime(async move {
            let mut received = 0u64;
            let bytes = update
                .download(
                    |chunk, total| {
                        received =
                            received.saturating_add(u64::try_from(chunk).unwrap_or(u64::MAX));
                        progress.post(received, total);
                    },
                    || {},
                )
                .await
                .map_err(|error| failure(&error))?;
            file_binding::bind(
                pubkey.as_deref(),
                &update.signature,
                &update.version,
                &bytes,
                target,
            )?;
            let version = offer.version.clone();
            let path = tauri::async_runtime::spawn_blocking(move || {
                save_download(&updates_dir, &version, &bytes)
            })
            .await
            .map_err(|_stopped| UpdaterError::Io("the download's write stopped".to_owned()))?
            .map_err(|error| {
                UpdaterError::Io(format!("the download was not saved: {}", error.kind()))
            })?;
            Ok(Downloaded {
                version: offer.version,
                path,
                handle: offer.handle,
            })
        })
    }
}

/// The target a check asks for first: the arm64 installer when `probe` moves an install of
/// `kind`, this build's own otherwise.
fn wanted(probe: Arm64Probe, kind: InstallKind) -> Target {
    if probe(kind) {
        Target::Arm64
    } else {
        Target::Native
    }
}

/// Checks `source` for `wanted`'s installer. When that is the arm64 one and the release has no
/// entry for it (`TargetNotFound`, `TargetsNotFound`), checks again for this build's own, x64
/// in place, before an absent entry counts as no update. The offer's [`Pending`] holds the
/// target of the check that returned it, so a fallback's install never uses the arm64 key.
/// `build` makes each check's updater ([`Source::updater`]; the tests' own client).
pub(super) async fn check_for(
    source: Source,
    wanted: Target,
    build: impl Fn(&Source, Target) -> Result<tauri_plugin_updater::Updater, PluginError>,
) -> Result<Option<Offer>, UpdaterError> {
    let mut target = wanted;
    let update = loop {
        let updater = build(&source, target).map_err(|error| failure(&error))?;
        match updater.check().await {
            Ok(update) => break update,
            Err(PluginError::TargetNotFound(_) | PluginError::TargetsNotFound(_))
                if target == Target::Arm64 =>
            {
                tracing::info!("the release has no arm64 installer: checking for x64 in place");
                target = Target::Native;
            }
            Err(error) => return no_entry_is_no_update(error),
        }
    };
    let Some(update) = update else {
        return Ok(None);
    };
    if target == Target::Arm64 {
        tracing::info!(
            target: super::install::LIFECYCLE,
            "updater: emulated x64 on ARM64; offering the arm64 build {}",
            update.version
        );
    }
    Ok(Some(Offer {
        version: update.version.clone(),
        handle: Opaque::new(Pending {
            update,
            source,
            target,
        }),
        refetch: false,
    }))
}

/// A check's error, where the release has no installer for this platform (`TargetNotFound`,
/// `TargetsNotFound`: a Linux build, or a release that left this one out) is no update, not a
/// failure.
pub(super) fn no_entry_is_no_update(error: PluginError) -> Result<Option<Offer>, UpdaterError> {
    match error {
        PluginError::TargetNotFound(_) | PluginError::TargetsNotFound(_) => {
            tracing::info!("the release has no installer for this platform: no update");
            Ok(None)
        }
        error => Err(failure(&error)),
    }
}

/// Runs `work` on Tauri's runtime, whose reqwest futures the host's pool cannot drive, and
/// returns a future over its answer. Dropping that future aborts the task, which drops the
/// request; one the task never answered is [`UpdaterError::Other`].
pub(super) fn on_runtime<T: Send + 'static>(
    work: impl Future<Output = Result<T, UpdaterError>> + Send + 'static,
) -> BoxFuture<'static, Result<T, UpdaterError>> {
    let (answer, answered) = oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        // The host stopped waiting: nothing to tell.
        let _unheard = answer.send(work.await);
    });
    let abort = AbortOnDrop(task);
    Box::pin(async move {
        let _abort = abort;
        answered.await.unwrap_or_else(|_canceled| {
            Err(UpdaterError::Other(
                "the updater's task ended without an answer".to_owned(),
            ))
        })
    })
}

/// Aborts its task when dropped (a no-op once the task has ended).
struct AbortOnDrop(tauri::async_runtime::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Saves `bytes`, the installer of `version`, as the only file of `<updates>/<version>/`, which
/// the host offers again at the next start (`find_cached`): written to a temporary file of its
/// own, `<updates>/.<version>.<uuid>.part` ([`temp_name`]), so two downloads of one version never
/// write the same file, and synced, then the version's folder emptied and the file moved in, so
/// the folder holds a whole file or none. A temporary file a crash left is removed by the host's
/// next start, which keeps nothing in `updates/` but the folder it offers. Refuses a version
/// that is not plain dotted numbers, the only form a
/// Clauseo release takes, so the folder's name is always one the host reads.
#[expect(
    clippy::disallowed_methods,
    reason = "the updater's one write: a temporary file in updates/, then a rename into the \
              version's folder; the desktop crate has no write module"
)]
pub(super) fn save_download(updates: &Path, version: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    if !is_plain_version(version) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the version is not plain dotted numbers",
        ));
    }
    fs::create_dir_all(updates)?;
    let temp = updates.join(temp_name(version));
    let written = fs::File::create(&temp).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    let folder = updates.join(version);
    let moved = written
        .and_then(|()| match fs::remove_dir_all(&folder) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        })
        .and_then(|()| fs::create_dir(&folder))
        .map(|()| folder.join(INSTALLER))
        .and_then(|path| fs::rename(&temp, &path).map(|()| path));
    match moved {
        Ok(path) => {
            sync_folder(&folder);
            Ok(path)
        }
        Err(error) => {
            // The write already failed; a temporary file left behind is only litter, which the
            // host's next start removes from updates/.
            drop(fs::remove_file(&temp));
            Err(error)
        }
    }
}

/// A download's temporary name in `updates/`: `.<version>.<uuid>.part`, a new UUID (v7: time and
/// random bits) each time. The leading dot keeps it from reading as a version's folder.
pub(super) fn temp_name(version: &str) -> String {
    format!(".{version}.{}.part", uuid::Uuid::now_v7().simple())
}

/// Syncs `folder`, so the rename into it survives a power cut (Unix; Windows has no folder
/// sync). A failure is logged by its kind: the file is there either way.
fn sync_folder(folder: &Path) {
    #[cfg(unix)]
    if let Err(error) = fs::File::open(folder).and_then(|dir| dir.sync_all()) {
        tracing::warn!(kind = %error.kind(), "the update's folder was not synced");
    }
    #[cfg(not(unix))]
    let _ = folder;
}

/// `0.4.1`: one or more parts of ASCII digits, split by dots.
pub(super) fn is_plain_version(version: &str) -> bool {
    version
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Installs `downloaded`, a download this updater made: rebuilds its update from the
/// `raw_json` the endpoint sent (`restore_update`, which requires a signed version), checks it
/// is still the same version, reads the file, refuses it unless its signature names this
/// platform's installer for the pending's target at that version (`file_binding::bind`), and
/// hands its bytes to the plugin's `install`, which verifies them again. On Windows it never
/// returns: the plugin starts the installer (`installMode: passive`) and exits. On macOS it
/// returns once the `.app` is swapped.
pub(super) fn install<R: Runtime>(
    app: &AppHandle<R>,
    downloaded: &Downloaded,
) -> Result<(), UpdaterError> {
    let pending = downloaded.handle.downcast_ref::<Pending>().ok_or_else(|| {
        UpdaterError::Other("the download did not come from this updater".to_owned())
    })?;
    let updater = pending
        .source
        .updater(app, pending.target)
        .map_err(|error| failure(&error))?;
    let update = updater
        .restore_update(pending.update.raw_json.clone())
        .map_err(|error| failure(&error))?
        .ok_or_else(|| UpdaterError::Other("the download is no longer an update".to_owned()))?;
    if update.version != downloaded.version {
        return Err(UpdaterError::Signature(
            "the restored update is not the downloaded version".to_owned(),
        ));
    }
    let bytes = fs::read(&downloaded.path).map_err(|error| {
        UpdaterError::Io(format!("the download was not read: {}", error.kind()))
    })?;
    let pubkey = pending.source.pubkey(app.config());
    file_binding::bind(
        pubkey.as_deref(),
        &update.signature,
        &update.version,
        &bytes,
        pending.target,
    )?;
    update.install(bytes).map_err(|error| failure(&error))
}

/// The host's error for a plugin error: its kind, and a fixed line that names no URL, path or
/// key. The plugin's own text is never kept: a request error's names the endpoint, and some
/// install errors name a folder.
pub(super) fn failure(error: &PluginError) -> UpdaterError {
    match error {
        PluginError::Reqwest(error) => UpdaterError::Network(request_failure(error)),
        PluginError::Network(_) => {
            UpdaterError::Network("the download answered an error status".to_owned())
        }
        PluginError::ReleaseNotFound => {
            UpdaterError::Network("no endpoint answered with a release".to_owned())
        }
        PluginError::Minisign(_) => {
            UpdaterError::Signature("the signature does not verify".to_owned())
        }
        PluginError::Base64(_) | PluginError::SignatureUtf8(_) => {
            UpdaterError::Signature("the signature or the key is not readable".to_owned())
        }
        PluginError::SignedVersionMismatch { .. } => UpdaterError::Signature(
            "the signature is for another version than the one offered".to_owned(),
        ),
        PluginError::MissingSignedVersion => {
            UpdaterError::Signature("the signature names no version".to_owned())
        }
        PluginError::Io(error) => UpdaterError::Io(format!("i/o: {}", error.kind())),
        PluginError::PreviousAppNotRestored(_) => {
            UpdaterError::Io("the update failed and the old app was not put back".to_owned())
        }
        PluginError::Serialization(_) | PluginError::Semver(_) => {
            UpdaterError::Other("the release manifest does not parse".to_owned())
        }
        PluginError::TargetNotFound(_) | PluginError::TargetsNotFound(_) => {
            UpdaterError::Other("the release has no installer for this platform".to_owned())
        }
        PluginError::EmptyEndpoints => UpdaterError::Other("no endpoint is set".to_owned()),
        PluginError::InsecureTransportProtocol => {
            UpdaterError::Other("the endpoint is not https".to_owned())
        }
        PluginError::UrlParse(_) => UpdaterError::Other("the endpoint is not a URL".to_owned()),
        PluginError::UnsupportedArch | PluginError::UnsupportedOs => {
            UpdaterError::Other("this platform cannot update itself".to_owned())
        }
        PluginError::FailedToDetermineExtractPath => {
            UpdaterError::Other("the app's install folder was not found".to_owned())
        }
        _ => UpdaterError::Other("the install failed".to_owned()),
    }
}

/// A request error's line: what failed, and the HTTP status when there was one, without the
/// URL reqwest's own text carries.
fn request_failure(error: &reqwest::Error) -> String {
    let what = if error.is_timeout() {
        "the request timed out"
    } else if error.is_connect() {
        "the endpoint could not be reached"
    } else if error.is_status() {
        "the endpoint answered an error status"
    } else if error.is_decode() || error.is_body() {
        "the answer could not be read"
    } else {
        "the request failed"
    };
    match error.status() {
        Some(status) => format!("{what} ({})", status.as_u16()),
        None => what.to_owned(),
    }
}
