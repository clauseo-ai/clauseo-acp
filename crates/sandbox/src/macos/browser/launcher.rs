//! [`BrowserLauncher`]: starts the browser stack under the browser profile through
//! `/usr/bin/sandbox-exec`, which applies the profile and execs the program in place, so the
//! pid, the process group and the tree kill stay `process`'s (as `SeatbeltLauncher` does).
//!
//! Chrome starts through [`BrowserLauncher::launch_chrome`], with `--remote-debugging-pipe`
//! and no `--remote-debugging-port`: the profile allows no inet listener, so `DevTools` leaves
//! Chrome on its fds 3 and 4, which `sandbox-exec` passes through to it (measured). The caller
//! gets this process's ends as a [`CdpPipe`]. Stage E's host bridge (J4), outside every jail,
//! is that caller: it launches Chrome here, keeps the [`ChromeLaunch`], and serves its pipe on
//! `127.0.0.1:<BrowserPaths::bridge_port>`, the one `DevTools` port agent-browser (started
//! here with [`Launcher::launch`]) may connect to. Each message on the pipe is one JSON text
//! ended by a NUL ([`super::pipe`]).

use std::ffi::{OsStr, OsString};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use clauseo_process::{LaunchSpec, Launched, Launcher, SpawnOptions, spawn_in_tree};

use super::pipe::{CHROME_IN_FD, CHROME_OUT_FD, CdpPipe, PIPE_FLAG};
use super::{BrowserPaths, BrowserProfileError, check, sandboxed, write};
use crate::macos::launcher::SANDBOX_EXEC;

/// Starts programs under one run's browser profile. Refuses to be made, and refuses each
/// launch, when this process already runs under a Seatbelt profile: the browser profile stands
/// in for Chrome's sandbox only outside the agent's jail (0038).
#[derive(Debug)]
pub struct BrowserLauncher {
    profile: String,
    run_dir: PathBuf,
    opts: SpawnOptions,
}

impl BrowserLauncher {
    /// A launcher for `paths`, starting programs with `opts` plus [`SpawnOptions::clear_env`]:
    /// a launched program gets its spec's variables plus `HOME` and `TMPDIR` (the run's `tmp/`)
    /// and `MAC_CHROMIUM_TMPDIR` (the run's `ab/`). Chrome takes its temp folder from
    /// `MAC_CHROMIUM_TMPDIR`, not `TMPDIR`, and binds its singleton socket there; without it
    /// Chrome uses the user's `/var/folders/...`, which the profile refuses, and exits (21).
    pub fn new(
        paths: &BrowserPaths,
        opts: SpawnOptions,
    ) -> Result<BrowserLauncher, BrowserProfileError> {
        refuse_nesting()?;
        let checked = check(paths)?;
        Ok(BrowserLauncher {
            profile: write(&checked, paths),
            run_dir: checked.run_dir,
            opts: opts.clear_env(),
        })
    }

    /// The profile every launch runs under.
    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// The spec `launch` starts for `spec`: `sandbox-exec -p <profile>`, then the program and
    /// its arguments; the spec's variables plus `HOME`, `TMPDIR` and `MAC_CHROMIUM_TMPDIR`; the
    /// same working folder and secrets.
    pub fn jailed_spec(&self, spec: &LaunchSpec) -> Result<LaunchSpec, clauseo_process::Error> {
        let mut args = vec![OsString::from("-p"), OsString::from(&self.profile)];
        args.push(OsString::from(spec.program()));
        args.extend(spec.args().iter().cloned());
        let mut env = spec.env().clone();
        for (name, sub) in [
            ("HOME", "tmp"),
            ("TMPDIR", "tmp"),
            ("MAC_CHROMIUM_TMPDIR", "ab"),
        ] {
            env.insert(
                OsString::from(name),
                self.run_dir.join(sub).into_os_string(),
            );
        }
        LaunchSpec::new(
            PathBuf::from(SANDBOX_EXEC),
            args,
            env,
            spec.cwd().to_owned(),
        )
        .map(|jailed| jailed.with_redacted(spec.redacted().iter().cloned()))
    }

    /// The run folder, canonical.
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Starts the Chrome `spec` names under the profile, with the arguments of [`chrome_args`]
    /// and a fresh pipe pair at its fds 3 (commands in) and 4 (replies out). This process keeps
    /// only its own ends; Chrome holds the only copies of the others, so each side sees the end
    /// of the stream when the other closes. Refused, as [`Launcher::launch`] is, inside a
    /// sandbox.
    pub fn launch_chrome(&self, spec: &LaunchSpec) -> Result<ChromeLaunch, clauseo_process::Error> {
        refuse_nesting().map_err(refused_inside)?;
        let pipe_error = |source| clauseo_process::Error::Os {
            what: "the DevTools pipe could not be made",
            source,
        };
        let (chrome_in, to_chrome) = std::io::pipe().map_err(pipe_error)?;
        let (from_chrome, chrome_out) = std::io::pipe().map_err(pipe_error)?;
        let chrome = LaunchSpec::new(
            spec.program().to_owned(),
            chrome_args(spec.args()),
            spec.env().clone(),
            spec.cwd().to_owned(),
        )?
        .with_redacted(spec.redacted().iter().cloned());
        // A clone for this launch only: its copies of Chrome's ends close when it drops below.
        let opts = self
            .opts
            .clone()
            .pass_fd(CHROME_IN_FD, OwnedFd::from(chrome_in))
            .pass_fd(CHROME_OUT_FD, OwnedFd::from(chrome_out));
        let launched = spawn_in_tree(&self.jailed_spec(&chrome)?, &opts)?;
        drop(opts);
        Ok(ChromeLaunch {
            launched,
            pipe: CdpPipe {
                to_chrome,
                from_chrome,
            },
        })
    }
}

/// A Chrome started by [`BrowserLauncher::launch_chrome`]: the process, and this process's
/// ends of its `DevTools` pipe.
#[derive(Debug)]
pub struct ChromeLaunch {
    /// Chrome's stdio, its tree and its handle, as for any launch.
    pub launched: Launched,
    /// Write commands to `pipe.to_chrome`, read replies and events from `pipe.from_chrome`.
    pub pipe: CdpPipe,
}

/// Chrome's arguments for a pipe launch: `--remote-debugging-pipe` first (before any `--`,
/// after which Chrome reads no switch), then `args` without any `remote-debugging-port`,
/// `remote-debugging-address` or `remote-debugging-pipe` switch, with one dash or two and
/// with or without a value, so `DevTools` never listens on a socket. Even one that got through
/// could not listen: the profile refuses every inet bind.
pub fn chrome_args(args: &[OsString]) -> Vec<OsString> {
    let dropped = |arg: &OsStr| {
        let arg = arg.to_string_lossy();
        let Some(name) = arg.strip_prefix('-') else {
            return false;
        };
        let name = name.strip_prefix('-').unwrap_or(name);
        [
            "remote-debugging-port",
            "remote-debugging-address",
            "remote-debugging-pipe",
        ]
        .iter()
        .any(|flag| {
            name.strip_prefix(flag)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('='))
        })
    };
    std::iter::once(OsString::from(PIPE_FLAG))
        .chain(args.iter().filter(|a| !dropped(a)).cloned())
        .collect()
}

fn refused_inside(error: BrowserProfileError) -> clauseo_process::Error {
    clauseo_process::Error::Os {
        what: "the browser profile refuses to start inside a sandbox",
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, error),
    }
}

impl Launcher for BrowserLauncher {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        refuse_nesting().map_err(refused_inside)?;
        spawn_in_tree(&self.jailed_spec(spec)?, &self.opts)
    }
}

/// The outermost check: [`BrowserProfileError::AlreadySandboxed`] when this process runs
/// under a Seatbelt profile.
pub fn refuse_nesting() -> Result<(), BrowserProfileError> {
    if sandboxed() {
        Err(BrowserProfileError::AlreadySandboxed)
    } else {
        Ok(())
    }
}
