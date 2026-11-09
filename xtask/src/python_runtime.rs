//! `cargo xtask python-runtime {check,bump,build}`: Clauseo's pinned Python runtime (`CPython` from
//! python-build-standalone plus a fixed set of document libraries), built per target from
//! `python-runtime/lock.toml` and published by `.github/workflows/python-runtime.yml`.
//!
//! - `check`: offline. The lock parses, every rule in `rules.rs` holds (four targets, no sdists,
//!   tags that fit each target and the macOS 13.0 floor, the licence allowlist, the closure), and
//!   `THIRD-PARTY-NOTICES.txt` was written from this lock. xtask's test `real_lock_passes` runs the
//!   same, so `cargo xtask check` (nextest) runs it too. `--online` is the nightly re-check
//!   (`online.rs`).
//! - `bump`: network. Re-resolves with pip and rewrites the lock and notices (`bump.rs`).
//! - `build`: network, on a machine of the target. Assembles and packs one archive (`build.rs`).
//!
//! See `python-runtime/README.md` for what each file is and how a bump goes.

mod assemble;
mod build;
mod bump;
mod lock;
mod net;
mod notices;
mod online;
mod rules;
mod target;
mod tree;
mod wheel;

use crate::output::Printer;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The lock, repo-relative.
pub(crate) const LOCK: &str = "python-runtime/lock.toml";
/// The notices, repo-relative; `build` copies it into each archive.
pub(crate) const NOTICES: &str = "python-runtime/THIRD-PARTY-NOTICES.txt";
/// The smoke test `build` runs with the finished runtime.
pub(crate) const SMOKE: &str = "python-runtime/smoke/smoke.py";
/// The smoke test's fixtures.
pub(crate) const SMOKE_DIR: &str = "python-runtime/smoke";
/// The script that packs the archive, run by the runtime's own Python.
pub(crate) const PACK: &str = "python-runtime/tools/pack.py";

/// The Python `bump` resolves with when `--resolver-python` is not given.
const DEFAULT_PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };

/// What `cargo xtask python-runtime` does.
#[derive(clap::Subcommand)]
pub(crate) enum Action {
    /// Check the lock and the notices offline; with `--online`, also re-check every hash against
    /// `PyPI` and PBS and ask OSV for advisories.
    Check {
        /// Re-check against `PyPI`, PBS and OSV (network).
        #[arg(long)]
        online: bool,
        /// With `--online`: write the advisories found as a JSON list to this file.
        #[arg(long, value_name = "FILE", requires = "online")]
        advisories: Option<PathBuf>,
    },
    /// Re-resolve the packages with pip for each target, take every hash from PBS and `PyPI`, and
    /// rewrite python-runtime/lock.toml and THIRD-PARTY-NOTICES.txt (network).
    Bump {
        /// The Python whose pip (23.1 or newer) resolves; nothing is installed into it.
        #[arg(long, value_name = "PYTHON", default_value = DEFAULT_PYTHON)]
        resolver_python: String,
        /// Keep the downloads in this folder (and reuse them on the next run) instead of a
        /// temporary one.
        #[arg(long, value_name = "DIR")]
        cache: Option<PathBuf>,
    },
    /// Build one target's archive into `--out`, on a machine of that target (network).
    Build {
        /// The target triple, e.g. aarch64-apple-darwin.
        #[arg(long)]
        target: String,
        /// The folder for the downloads, the stage and the archive.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
        /// Only download the inputs and check them; runs on any machine.
        #[arg(long)]
        download_only: bool,
        /// Skip checking PBS's build attestation with gh. For a local try only; CI never passes
        /// it.
        #[arg(long)]
        no_attestation: bool,
    },
}

/// A failure: what went wrong, and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub message: String,
    pub fix: String,
}

impl Failure {
    pub(crate) fn new(message: impl Into<String>, fix: impl Into<String>) -> Failure {
        Failure {
            message: message.into(),
            fix: fix.into(),
        }
    }

    /// A failed file operation: `<what>: <error>`.
    pub(crate) fn io(what: &str, error: &io::Error) -> Failure {
        Failure::new(
            format!("{what}: {error}"),
            "check the path and its permissions, then run it again",
        )
    }
}

/// Runs `action` in the repo at `root`.
pub(crate) fn run(root: &Path, action: Action) -> ExitCode {
    let mut out = Printer::stdout();
    match dispatch(root, action, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            out.failure(&failure.message, &failure.fix);
            ExitCode::FAILURE
        }
    }
}

fn dispatch(root: &Path, action: Action, out: &mut Printer<io::Stdout>) -> Result<(), Failure> {
    match action {
        Action::Check { online, advisories } => {
            let lock = check(root)?;
            out.note(format!(
                "check: {LOCK} and {NOTICES} pass ({} packages, release {})",
                lock.packages.len(),
                lock.release_tag()
            ));
            if !online {
                return Ok(());
            }
            let work = std::env::temp_dir().join(format!(
                "xtask-python-runtime-online-{}",
                std::process::id()
            ));
            crate::write::make_dirs(&work)
                .map_err(|error| Failure::io("making a scratch folder", &error))?;
            let advisories = match advisories {
                Some(path) => Some(
                    std::path::absolute(&path)
                        .map_err(|error| Failure::io("resolving --advisories", &error))?,
                ),
                None => None,
            };
            let result = online::run(&lock, &work, advisories.as_deref(), out);
            let (dir, name) = tree::split(&work)?;
            net::remove(&dir, &name)?;
            result
        }
        Action::Bump {
            resolver_python,
            cache,
        } => {
            let options = bump::Options {
                resolver: resolver_python,
                cache,
            };
            bump::run(root, out, &options)
        }
        Action::Build {
            target,
            out: out_dir,
            download_only,
            no_attestation,
        } => {
            let lock = check(root)?;
            let target = target::Target::from_triple(&target).ok_or_else(|| {
                Failure::new(
                    format!("`{target}` is not one of the runtime's targets"),
                    format!(
                        "pass one of {}",
                        target::TARGETS.map(target::Target::triple).join(", ")
                    ),
                )
            })?;
            let options = build::Options {
                target,
                out: out_dir,
                download_only,
                no_attestation,
            };
            build::run(root, &lock, &options, out)
        }
    }
}

/// The lock under `root`, once it parses and passes every offline check with its notices.
pub(crate) fn check(root: &Path) -> Result<lock::Lock, Failure> {
    let read = |rel: &str| {
        std::fs::read_to_string(root.join(rel))
            .map_err(|error| Failure::io(&format!("reading {rel}"), &error))
    };
    let text = read(LOCK)?;
    let lock = lock::Lock::parse(&text).map_err(|error| {
        Failure::new(
            format!("{LOCK} does not parse: {error}"),
            "fix the lock; see python-runtime/README.md",
        )
    })?;
    let mut found = rules::problems(&lock);
    found.extend(notices::problems(&read(NOTICES)?, &lock, &text));
    if lock.render() != text.replace("\r\n", "\n") {
        found.push(format!("{LOCK} is not laid out as bump writes it"));
    }
    if found.is_empty() {
        Ok(lock)
    } else {
        Err(Failure::new(
            format!("{LOCK}:\n{}", found.join("\n")),
            "change [runtime] or [resolve] by hand and run `cargo xtask python-runtime bump`; \
             never edit a hash, size or file name by hand",
        ))
    }
}

#[cfg(test)]
mod tests;
