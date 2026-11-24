//! The OS behind [`Probe`]: the settings' matters folder, a child process run to its end (macOS'
//! Documents ask), and the macOS and Windows reads (`mac.rs`, `windows.rs`, the latter in this
//! process), with nothing to read elsewhere.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::{ErrorCode, HostError};
use clauseo_settings::{Locations, PlatformDirs as _, SystemDirs, SystemEnv};

use super::pick::{Mount, parse_mounts};
use super::{Cfa, KeychainCheck, KeychainItem, Platform, Probe, cfa_from_layers};

/// A child's exit code and stdout; `None` when it outlasted its limit.
type Ran = Option<(Option<i32>, Vec<u8>)>;

/// The most of a child's stdout kept; the rest is read and dropped, so the child never stalls.
const OUTPUT_MAX: u64 = 1024 * 1024;

/// Runs `program args` through the process crate (its tree, `CREATE_NO_WINDOW` on Windows) and
/// answers its exit code and stdout; `Ok(None)` when it outlasts `limit` (its tree is killed).
/// Blocks: called on the commands' blocking thread.
pub(super) fn run(
    program: &Path,
    args: &[&std::ffi::OsStr],
    limit: Option<Duration>,
) -> Result<Ran, clauseo_process::Error> {
    use std::pin::pin;

    use clauseo_process::{AsyncIoTimer, Launcher as _, Timer as _};
    use futures::AsyncReadExt as _;
    use futures::future::{Either, join3, select};

    let spec = clauseo_process::LaunchSpec::new(
        program.to_owned(),
        args.iter().map(|arg| (*arg).to_owned()).collect(),
        std::collections::BTreeMap::new(),
        std::env::temp_dir(),
    )?;
    let launcher = clauseo_process::PassthroughLauncher::new(clauseo_process::SpawnOptions::new());
    let clauseo_process::Launched {
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
            let (read, drained, report) = join3(read, stderr.drain(), handle.exited()).await;
            drop(drained);
            (read.unwrap_or_default(), report.code())
        });
        let timeout = async {
            match limit {
                Some(limit) => AsyncIoTimer.after(limit).await,
                None => futures::future::pending().await,
            }
        };
        match select(work, pin!(timeout)).await {
            Either::Left(((out, code), _)) => Some((code, out)),
            Either::Right(((), _)) => None,
        }
    });
    drop(handle);
    Ok(ran)
}

/// Clauseo's keychain services, as the `keyring` crate writes them: the agents' keys
/// (`clauseo_agents`' `keys.rs`), the connectors' (`clauseo_connectors`' `known.rs`) and the
/// MCP servers' (`clauseo_settings`' `mcp.rs`).
pub(crate) const KEYCHAIN_SERVICES: [&str; 3] = [
    "ai.clauseo.desktop",
    "ai.clauseo.desktop.connectors",
    "ai.clauseo.desktop.mcp",
];

#[cfg(target_os = "macos")]
use super::mac::{accounts, read, silent_read};

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

/// No keychain items to read outside macOS.
#[cfg(not(target_os = "macos"))]
fn read(_service: &str, _account: &str) -> bool {
    false
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
    match run(Path::new("/sbin/mount"), &[], Some(MOUNT_TIMEOUT)) {
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
        let ran = run(Path::new("/bin/ls"), &[folder.as_os_str()], None).map_err(|error| {
            HostError::new(
                ErrorCode::Internal,
                format!("the folder's listing did not start: {error}"),
            )
        })?;
        // `ls` exits 1 with "Operation not permitted" when macOS refuses.
        Ok(matches!(ran, Some((Some(0), _))))
    }

    fn read_keychain(&self, item: &KeychainItem) -> bool {
        read(&item.service, &item.account)
    }

    fn mounts(&self) -> Vec<Mount> {
        if cfg!(target_os = "macos") {
            mount_table()
        } else {
            Vec::new()
        }
    }
}
