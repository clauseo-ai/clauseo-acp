//! One repair, in order, over [`Seams`] (the app's on Windows, `windows.rs`; the tests' stubs):
//!
//! 1. the attempt's folder and file ([`Seams::stage`]), the installer's `.sig`, then the installer
//!    streamed into the file through its handle;
//! 2. the file sealed ([`Staged::seal`]: from here no writer can open it) and read back through
//!    that same handle, and only those bytes checked: the minisign signature under the updater
//!    key, global signature included, naming exactly this version's per-user installer for this
//!    architecture and this version (`file_binding::check_update_file`, #728's check);
//! 3. the updater's state read again ([`Seams::still_eligible`]: the download can take minutes,
//!    and an update scheduled or installing meanwhile refuses), "Windows will ask for permission"
//!    to the window, the `repair-pending` marker written, and every window destroyed
//!    ([`Seams::hand_over`]): the installer refuses while Clauseo's main window exists;
//! 4. unless the app is quitting ([`Seams::quitting`]), the installer started with exactly
//!    [`ARGS`], while the handle still holds the file; then the handle closed and the app's quit
//!    started ([`Seams::exit`]).
//!
//! Any failure discards the attempt (the handle closed, the folder deleted) and answers its
//! code; one after the windows went brings the main window back ([`Seams::bring_back`]).

use std::future::Future;
use std::io;
use std::pin::pin;
use std::time::{Duration, Instant};

use futures::future::Either;

use clauseo_protocol::{ErrorCode, HostError};

use super::super::file_binding::{check_update_file, line};
use super::super::install::LIFECYCLE;
use super::fetch;
use super::plan::Plan;
use super::{RepairEnd, RepairStep};

/// How long the `.sig` and the installer may take together before the download is given up
/// (`repair.download_failed`): several times what today's installer (about 21 MB) needs on a
/// slow line, short enough that a stalled one never leaves the card waiting for good.
pub(super) const DOWNLOAD_BUDGET: Duration = Duration::from_mins(10);

/// The installer's arguments: interactive (never `/S`: the lawyer sees the installer), and
/// stage F's switch, which makes it wait for Clauseo to be gone before its app-running checks
/// and start Clauseo again once at its end.
pub(super) const ARGS: [&str; 1] = ["/CLAUSEO_REPAIR"];

/// Why a repair stopped: each is its own code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// `repair.not_eligible`.
    NotEligible,
    /// `repair.download_failed`.
    DownloadFailed,
    /// `repair.verify_failed`.
    VerifyFailed,
    /// `repair.launch_failed`.
    LaunchFailed,
}

impl Refusal {
    /// The command's failure: the code, and a fixed line with no path or URL.
    pub(crate) fn error(self) -> HostError {
        let (code, message) = match self {
            Refusal::NotEligible => (
                ErrorCode::RepairNotEligible,
                "the installer can't be run again from here",
            ),
            Refusal::DownloadFailed => (
                ErrorCode::RepairDownloadFailed,
                "the installer was not downloaded",
            ),
            Refusal::VerifyFailed => (
                ErrorCode::RepairVerifyFailed,
                "the installer did not pass the signature check",
            ),
            Refusal::LaunchFailed => (ErrorCode::RepairLaunchFailed, "the installer did not start"),
        };
        HostError::new(code, message)
    }
}

/// The attempt's file, from its creation to the launch: one chain of handles, never reopened by
/// path.
pub(super) trait Staged {
    /// Appends a downloaded chunk.
    fn write(&mut self, chunk: &[u8]) -> io::Result<()>;
    /// Ends writing: from here the file is held so that nothing can open it to write, rename or
    /// delete it, until the stage is closed.
    fn seal(&mut self) -> io::Result<()>;
    /// The whole file, read from its start through the sealed handle.
    fn read_back(&mut self) -> io::Result<Vec<u8>>;
}

/// What the repair does to the machine and the app.
pub(super) trait Seams {
    /// The attempt's file.
    type Stage: Staged;
    /// A new attempt: a fresh folder of the lawyer's alone, and the file in it, created new.
    fn stage(&self, file: &str) -> io::Result<Self::Stage>;
    /// Closes the handle and deletes the attempt's folder.
    fn discard(&self, stage: Self::Stage);
    /// Tells the window where the repair is.
    fn announce(&self, step: RepairStep);
    /// Whether the updater still lets the repair run, asked again just before the hand-over: no
    /// update scheduled or installing, updates not managed, and the host answered.
    fn still_eligible(&self) -> bool;
    /// Writes the `repair-pending` marker the next start reads.
    fn mark(&self) -> io::Result<()>;
    /// Deletes the marker.
    fn unmark(&self);
    /// Destroys every window; true once none is left.
    fn hand_over(&self) -> bool;
    /// Starts the installer held by `stage` with `args`, and returns once the start returned.
    fn launch(&self, stage: &Self::Stage, args: &[&str]) -> io::Result<()>;
    /// Closes the handle, keeping the folder: the installer runs from it. The next start
    /// sweeps it.
    fn close(&self, stage: Self::Stage);
    /// Builds the main window again, saying the installer did not start.
    fn bring_back(&self);
    /// Whether the app's quit has started (the lawyer quit meanwhile): then nothing launches.
    fn quitting(&self) -> bool;
    /// Quits the app, bounded, so the installer can replace it.
    fn exit(&self);
}

/// Runs one repair of `plan` over `seams`, downloading with `client`. `Ok` means the installer
/// started and the app is quitting.
pub(super) async fn run<S: Seams>(
    seams: &S,
    client: &reqwest::Client,
    plan: &Plan,
    budget: Duration,
    cancel: impl Future<Output = ()>,
) -> Result<RepairEnd, Refusal> {
    tracing::info!(
        target: LIFECYCLE,
        version = plan.version,
        file = plan.file,
        "repair: downloading this version's installer"
    );
    seams.announce(RepairStep::Downloading);
    let mut stage = seams.stage(&plan.file).map_err(|error| {
        tracing::warn!(target: LIFECYCLE, kind = %error.kind(), "repair: no folder for the download");
        Refusal::DownloadFailed
    })?;
    // The `.sig` and the installer together get `budget`; a cancel ends either at once.
    let deadline = crate::open::now() + budget;
    let downloaded = {
        let download = pin!(download(&mut stage, client, plan, deadline));
        match futures::future::select(download, pin!(cancel)).await {
            Either::Left((result, _cancel)) => Some(result),
            Either::Right(((), _download)) => None,
        }
    };
    let signature = match downloaded {
        Some(Ok(signature)) => signature,
        None => {
            seams.discard(stage);
            seams.unmark();
            tracing::info!(target: LIFECYCLE, "repair: cancelled during the download; deleted");
            return Ok(RepairEnd::Cancelled);
        }
        Some(Err(failed)) => {
            seams.discard(stage);
            tracing::warn!(target: LIFECYCLE, what = failed.0, "repair: the download failed");
            return Err(Refusal::DownloadFailed);
        }
    };
    if let Err(what) = verify(&mut stage, plan, &signature) {
        seams.discard(stage);
        tracing::warn!(
            target: LIFECYCLE,
            what,
            "repair: the installer failed the check; deleted, not run"
        );
        return Err(Refusal::VerifyFailed);
    }
    tracing::info!(target: LIFECYCLE, version = plan.version, "repair: the installer verified");
    hand_over_and_launch(seams, stage).map(|()| RepairEnd::Started)
}

/// The `.sig`, then the installer into `stage`.
async fn download<S: Staged>(
    stage: &mut S,
    client: &reqwest::Client,
    plan: &Plan,
    deadline: Instant,
) -> Result<String, fetch::Failed> {
    let signature = fetch::signature(client, &plan.signature, deadline).await?;
    let bytes = fetch::installer(client, &plan.installer, deadline, |chunk| {
        stage.write(chunk)
    })
    .await?;
    tracing::info!(target: LIFECYCLE, bytes, "repair: the installer downloaded");
    Ok(signature)
}

/// Seals `stage`, reads it back through its handle, and checks those bytes against `signature`
/// as `plan`'s file and version. The error is a fixed line.
fn verify<S: Staged>(stage: &mut S, plan: &Plan, signature: &str) -> Result<(), &'static str> {
    stage
        .seal()
        .map_err(|_held| "the file could not be held for the check")?;
    let bytes = stage
        .read_back()
        .map_err(|_unread| "the file could not be read back")?;
    check_update_file(&plan.pubkey, signature, &bytes, &plan.file, &plan.version)
        .map_err(|error| line(&error))
}

/// The updater asked again, the marker, the windows destroyed, the launch through the held
/// handle, then the handle closed and the quit; or, when the launch fails, everything taken back
/// and the window rebuilt. A quit that started meanwhile launches nothing and rebuilds nothing.
fn hand_over_and_launch<S: Seams>(seams: &S, stage: S::Stage) -> Result<(), Refusal> {
    if !seams.still_eligible() {
        seams.discard(stage);
        tracing::info!(
            target: LIFECYCLE,
            "repair: an update is installing or managed now; deleted, not run"
        );
        return Err(Refusal::NotEligible);
    }
    seams.announce(RepairStep::Launching);
    if let Err(error) = seams.mark() {
        // Without it the next start only misses the note it would show.
        tracing::warn!(target: LIFECYCLE, kind = %error.kind(), "repair: the marker was not written");
    }
    let handed = seams.hand_over();
    if seams.quitting() {
        seams.discard(stage);
        seams.unmark();
        tracing::info!(target: LIFECYCLE, "repair: the app is quitting; deleted, not run");
        return Err(Refusal::LaunchFailed);
    }
    let launched = if handed {
        seams.launch(&stage, &ARGS)
    } else {
        Err(io::Error::other("a window was still open"))
    };
    match launched {
        Ok(()) => {
            seams.close(stage);
            tracing::info!(target: LIFECYCLE, "repair: the installer started; quitting");
            seams.exit();
            Ok(())
        }
        Err(error) => {
            seams.discard(stage);
            seams.unmark();
            tracing::warn!(
                target: LIFECYCLE,
                kind = %error.kind(),
                "repair: the installer did not start; deleted, window back"
            );
            seams.bring_back();
            Err(Refusal::LaunchFailed)
        }
    }
}
