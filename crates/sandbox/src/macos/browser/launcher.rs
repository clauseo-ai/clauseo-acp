//! [`BrowserLauncher`]: starts the browser stack under the browser profile through
//! `/usr/bin/sandbox-exec`, which applies the profile and execs the program in place, so the
//! pid, the process group and the tree kill stay `process`'s (as `SeatbeltLauncher` does).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_process::{LaunchSpec, Launched, Launcher, SpawnOptions, spawn_in_tree};

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
}

impl Launcher for BrowserLauncher {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        refuse_nesting().map_err(|error| clauseo_process::Error::Os {
            what: "the browser profile refuses to start inside a sandbox",
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, error),
        })?;
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
