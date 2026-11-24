//! The OS behind [`Probe`]: the settings' matters folder, a child process run to its end (macOS'
//! Documents ask), and the macOS and Windows reads (`mac.rs`, `windows.rs`, the latter in this
//! process), with nothing to read elsewhere.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::sync::Mutex;

use futures::channel::oneshot;

use crate::bridge::lock;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use clauseo_protocol::{ErrorCode, HostError};
use clauseo_settings::{Locations, PlatformDirs as _, SystemDirs, SystemEnv};

use super::ask_child::{AskResult, FLAG, request_lines, results};
use super::pick::{Mount, parse_mounts};
use super::{Cfa, KeychainCheck, KeychainItem, Platform, Probe, cfa_from_layers};

/// A child's exit code and stdout; `None` when it outlasted its limit.
pub(super) type Ran = Option<(Option<i32>, Vec<u8>)>;

/// The most of a child's stdout kept; the rest is read and dropped, so the child never stalls.
const OUTPUT_MAX: u64 = 1024 * 1024;

/// A child's environment and how it may be ended early, beyond its program and arguments.
#[derive(Default)]
pub(super) struct ChildOpts<'a> {
    /// Written to its stdin, which is then closed.
    pub(super) input: &'a [u8],
    /// Its whole environment, when set; else it inherits this process's.
    pub(super) env: Option<BTreeMap<OsString, OsString>>,
    /// The longest it may run.
    pub(super) limit: Option<Duration>,
    /// Ends it at once when it fires (or its sender drops): the quit's ([`AskStop`]).
    pub(super) stop: Option<oneshot::Receiver<()>>,
}

/// Runs `program args` through the process crate (its tree, `CREATE_NO_WINDOW` on Windows),
/// as `opts` says, and answers its exit code and stdout; `Ok(None)` when it outlasts its limit
/// or its stop fires (its tree is killed: the handle kills it when it drops). Blocks: called on
/// the commands' blocking thread.
pub(super) fn run(
    program: &Path,
    args: &[&std::ffi::OsStr],
    opts: ChildOpts<'_>,
) -> Result<Ran, clauseo_process::Error> {
    use std::pin::pin;

    use clauseo_process::{AsyncIoTimer, Launcher as _, Timer as _};
    use futures::future::{Either, join4, select};
    use futures::{AsyncReadExt as _, AsyncWriteExt as _};

    let ChildOpts {
        input,
        env,
        limit,
        stop,
    } = opts;
    let spawn = match env {
        Some(_) => clauseo_process::SpawnOptions::new().clear_env(),
        None => clauseo_process::SpawnOptions::new(),
    };
    let spec = clauseo_process::LaunchSpec::new(
        program.to_owned(),
        args.iter().map(|arg| (*arg).to_owned()).collect(),
        env.unwrap_or_default(),
        std::env::temp_dir(),
    )?;
    let launcher = clauseo_process::PassthroughLauncher::new(spawn);
    let clauseo_process::Launched {
        mut stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = launcher.launch(&spec)?;
    let ran = futures::executor::block_on(async {
        let work = pin!(async {
            let read = async {
                let mut out = Vec::new();
                (&mut stdout).take(OUTPUT_MAX).read_to_end(&mut out).await?;
                futures::io::copy(&mut stdout, &mut futures::io::sink()).await?;
                Ok::<_, std::io::Error>(out)
            };
            let write = async {
                // A child that ends without reading it all closes the pipe: nothing to do.
                let _closed = stdin.write_all(input).await;
                let _closed = stdin.close().await;
            };
            let (read, drained, report, ()) =
                join4(read, stderr.drain(), handle.exited(), write).await;
            drop(drained);
            (read.unwrap_or_default(), report.code())
        });
        let timeout = async {
            match limit {
                Some(limit) => AsyncIoTimer.after(limit).await,
                None => futures::future::pending().await,
            }
        };
        let stopped = async {
            match stop {
                Some(stop) => drop(stop.await),
                None => futures::future::pending().await,
            }
        };
        let timeout = async { select(pin!(timeout), pin!(stopped)).await.factor_first().0 };
        match select(work, pin!(timeout)).await {
            Either::Left(((out, code), _)) => Some((code, out)),
            Either::Right(((), _)) => None,
        }
    });
    drop(handle);
    Ok(ran)
}

/// Every keychain service Clauseo writes, by the constants their writers use, so the keychain
/// row's check and its Allow cover each one: the agents' keys (read at start), the connectors'
/// credentials (the case-law one at host start), the MCP servers' secrets (at each launch) and
/// the blocked-site allow-list's integrity hash (at each host start; left out, a refused read
/// there would stay refused for good and a revoke would void the hash).
pub(crate) const KEYCHAIN_SERVICES: [&str; 4] = [
    clauseo_agents::keys::KEYCHAIN_SERVICE,
    clauseo_connectors::KEYCHAIN_SERVICE,
    clauseo_settings::MCP_KEYCHAIN_SERVICE,
    crate::blocklist::KEYCHAIN_SERVICE,
];

#[cfg(target_os = "macos")]
pub(super) use super::mac::read;
#[cfg(target_os = "macos")]
use super::mac::{accounts, silent_read};

/// No keychain items to read outside macOS.
#[cfg(not(target_os = "macos"))]
fn accounts(_service: &str) -> Vec<String> {
    Vec::new()
}

/// No keychain items to read outside macOS.
#[cfg(not(target_os = "macos"))]
fn silent_read(_service: &str, _account: &str) -> Option<bool> {
    None
}

/// No keychain to read outside macOS: `errSecUnimplemented`.
#[cfg(not(target_os = "macos"))]
pub(super) fn read(_service: &str, _account: &str) -> Result<bool, i32> {
    Err(-4)
}

#[cfg(windows)]
use super::windows::{cfa_layers, protected_defaults};

/// No Defender outside Windows: no place to read, so it cannot be told.
#[cfg(not(windows))]
fn cfa_layers() -> (Vec<super::CfaLayer>, Vec<PathBuf>) {
    (Vec::new(), Vec::new())
}

/// No Defender outside Windows.
#[cfg(not(windows))]
fn protected_defaults() -> Vec<PathBuf> {
    Vec::new()
}

/// The longest `/sbin/mount` may take to list the mounts before a pick gives up on them.
const MOUNT_TIMEOUT: Duration = Duration::from_secs(5);

/// The mount table, from `/sbin/mount` (macOS): it lists the kernel's mounts and opens no volume.
/// Empty when it fails, exits non-zero or outlasts [`MOUNT_TIMEOUT`] (logged), so a pick then
/// asks for no permission.
fn mount_table() -> Vec<Mount> {
    let opts = ChildOpts {
        limit: Some(MOUNT_TIMEOUT),
        ..ChildOpts::default()
    };
    match run(Path::new("/sbin/mount"), &[], opts) {
        Ok(Some((Some(0), listing))) => parse_mounts(&String::from_utf8_lossy(&listing)),
        Ok(_) => {
            tracing::warn!("permissions: /sbin/mount gave no mount table");
            Vec::new()
        }
        Err(error) => {
            tracing::warn!(%error, "permissions: /sbin/mount did not start");
            Vec::new()
        }
    }
}

/// The longest the keychain Ask's child may wait on macOS's dialog: past it its tree is killed
/// and the row answers denied, so `asking` never lasts for ever.
const ASK_LIMIT: Duration = Duration::from_mins(5);

/// The quit's way to end the keychain Ask's child ([`end_keychain_ask`]).
static ASK_STOP: AskStop = AskStop::new();

/// Ends the keychain Ask's child, if one runs, and any started after: the quit calls it, so the
/// child and its dialog never outlive the app (`quit.rs`).
pub(crate) fn end_keychain_ask() {
    ASK_STOP.stop();
}

/// The quit's stop for the running Ask's child: one registered at a time (one Ask runs at a
/// time), fired by [`AskStop::stop`]; once stopped, a child registered later is stopped at once.
pub(super) struct AskStop {
    state: Mutex<(Option<oneshot::Sender<()>>, bool)>,
}

impl AskStop {
    pub(super) const fn new() -> Self {
        Self {
            state: Mutex::new((None, false)),
        }
    }

    /// The stop for a child about to start; it fires when [`AskStop::stop`] runs.
    pub(super) fn register(&self) -> oneshot::Receiver<()> {
        let (fire, fired) = oneshot::channel();
        let mut state = lock(&self.state);
        if !state.1 {
            // An earlier child's sender, if any, is replaced: that child has ended (one Ask
            // runs at a time).
            state.0 = Some(fire);
        }
        // Stopped already: `fire` drops here, so `fired` resolves at once.
        fired
    }

    /// Stops the registered child, and every one registered after.
    pub(super) fn stop(&self) {
        let mut state = lock(&self.state);
        state.1 = true;
        if let Some(fire) = state.0.take() {
            // The child may have ended already; nothing left to stop then.
            let _ended = fire.send(());
        }
    }
}

/// The Ask child's whole environment: `HOME` and nothing else. security-framework finds the
/// login keychain from the home folder, so the child reads the same keychain as this process;
/// nothing else of this process's environment (a `DYLD_` variable, a proxy) reaches it.
pub(super) fn child_env() -> BTreeMap<OsString, OsString> {
    std::env::var_os("HOME")
        .map(|home| (OsString::from("HOME"), home))
        .into_iter()
        .collect()
}

/// Whether the keychain Ask's child runs now (`keychain.rs`).
static KEYCHAIN_ASKING: AtomicBool = AtomicBool::new(false);

/// The OS itself.
pub(super) struct SystemProbe;

impl Probe for SystemProbe {
    fn platform(&self) -> Platform {
        if cfg!(target_os = "macos") {
            Platform::Mac
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Other
        }
    }

    fn matters_folder(&self) -> Option<PathBuf> {
        let locations = Locations::resolve_without_documents(&SystemDirs, &SystemEnv)
            .ok()?
            .with_documents(&SystemDirs, &SystemEnv);
        crate::host_start::load_effective(&locations)
            .default_matters_dir()
            .map(Path::to_path_buf)
    }

    fn documents(&self) -> Option<PathBuf> {
        SystemDirs.home().map(|home| home.join("Documents"))
    }

    fn keychain(&self) -> KeychainCheck {
        let mut check = KeychainCheck::default();
        for service in KEYCHAIN_SERVICES {
            for account in accounts(service) {
                let readable = silent_read(service, &account);
                let item = KeychainItem {
                    service: service.to_owned(),
                    account,
                };
                match readable {
                    Some(true) => {}
                    Some(false) => check.would_ask.push(item),
                    None => check.unknown.push(item),
                }
            }
        }
        check
    }

    fn controlled_folder_access(&self) -> Option<Cfa> {
        let (layers, protected) = cfa_layers();
        cfa_from_layers(&layers, protected)
    }

    fn protected_defaults(&self) -> Vec<PathBuf> {
        protected_defaults()
    }

    fn list_folder(&self, folder: &Path) -> Result<bool, HostError> {
        let ran = run(
            Path::new("/bin/ls"),
            &[folder.as_os_str()],
            ChildOpts::default(),
        )
        .map_err(|error| {
            HostError::new(
                ErrorCode::Internal,
                format!("the folder's listing did not start: {error}"),
            )
        })?;
        // `ls` exits 1 with "Operation not permitted" when macOS refuses.
        Ok(matches!(ran, Some((Some(0), _))))
    }

    fn ask_keychain(&self, items: &[KeychainItem]) -> Result<Vec<AskResult>, HostError> {
        let failed =
            |what: String| HostError::new(ErrorCode::Internal, format!("the keychain Ask {what}"));
        let exe =
            std::env::current_exe().map_err(|error| failed(format!("found no app: {error}")))?;
        let input = request_lines(items);
        let opts = ChildOpts {
            input: input.as_bytes(),
            env: Some(child_env()),
            limit: Some(ASK_LIMIT),
            stop: Some(ASK_STOP.register()),
        };
        let ran = run(&exe, &[OsStr::new(FLAG)], opts)
            .map_err(|error| failed(format!("did not start: {error}")))?;
        let Some((code, output)) = ran else {
            // Past the limit, or the quit: killed. No answer, so what still asks reads denied.
            tracing::info!("permissions: keychain Ask ended by its limit or the quit");
            return Ok(Vec::new());
        };
        let answers = results(&String::from_utf8_lossy(&output));
        tracing::info!(
            ?code,
            items = items.len(),
            answers = answers.len(),
            "permissions: keychain Ask ended"
        );
        Ok(answers)
    }

    fn keychain_asking(&self) -> &AtomicBool {
        &KEYCHAIN_ASKING
    }

    fn mounts(&self) -> Vec<Mount> {
        if cfg!(target_os = "macos") {
            mount_table()
        } else {
            Vec::new()
        }
    }
}
