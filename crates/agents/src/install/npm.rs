//! A try's install step: Node's runtime unpacked when it was downloaded, then the derived
//! `package.json` and a lock whose `resolved` URLs are `file:` URLs of the checked tarballs
//! written to `install/.tmp-<hash>-<n>`, `npm ci --offline` there, a walk that refuses any link,
//! the rename to `<hash>` and the receipt, written last.
//!
//! `npm ci` reads each tarball in place (in `downloads/`, or in the offline bundle) and checks
//! it against the entry's `integrity`, on top of the download's own check: a file swapped in
//! between is `EINTEGRITY`. Every entry npm installs on this platform must have its tarball,
//! or the try stops (a deterministic failure, not retried). Entries for other platforms keep
//! their registry `resolved`: they are inert, since npm skips them and `--offline` would refuse
//! to fetch them. Every other byte of the lock is the snapshot's, so the receipt's `<hash>` is
//! unchanged. There is no `npm cache add` step (decision 0033, 2026-11-04).
//!
//! Every npm run starts through the launcher as `node --single-threaded <npm-cli.js> …` (no V8
//! background tasks, so Node cannot deadlock as it exits: `NODE_SINGLE_THREADED`), with
//! neither `NODE_OPTIONS` nor `NODE_PATH` (the spawn drops both), the private cache, and empty
//! `agents/npmrc` and `agents/npmrc-global` files as its user and global config (npm refuses
//! one file for both).
//! `npm ci` runs with `--offline --ignore-scripts --no-bin-links --no-audit --no-fund` and
//! `--allow-file=all` (npm 11 reads `npm_config_allow_file` from the environment the launch
//! inherits, and `none` there would refuse the `file:` tarballs; npm 10 accepts the flag).
//! No proxy or CA file is passed: npm never uses the network here (every tarball is a local
//! file and `ci` runs `--offline`), and a proxy's password on a command line is visible to
//! other local users. Its stderr pump and its stdout are driven to their end while its exit is
//! raced against a pause (the handle is dropped, which kills its tree) and 10 minutes. Nothing
//! of its output reaches the log but npm's own `npm error code X` line, redacted.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Timer};
use futures::AsyncReadExt as _;
use futures::future::{Either, join3, select};

use super::files::{Fetched, npm_cache};
use super::{Job, NodeChoice, Run, TryError};
use crate::config::AgentsConfig;
use crate::node::{self, NodeSource, node_line};
use crate::receipt::{Receipt, install_folder, install_root, script_path, write_receipt};
use crate::state::{InstallState, Installed};
use crate::write;

/// The longest one npm run may take.
const NPM_LIMIT: Duration = Duration::from_mins(10);
/// V8's flag that turns off its background tasks, given to the Node that runs npm before
/// `npm-cli.js`. Node can deadlock as it exits when a background compile job needs a GC on the
/// main thread while that thread waits for the job (nodejs/node#54918, open in Node 24): npm has
/// finished its work, and the process never ends. On a loaded machine about one `npm ci` in a
/// few thousand did that; with no background tasks there is nothing to wait for. npm does not
/// need them: its run is short and mostly file work.
const NODE_SINGLE_THREADED: &str = "--single-threaded";
/// The flags `npm ci` runs with, besides the common ones.
const CI_FLAGS: [&str; 6] = [
    "--offline",
    "--ignore-scripts",
    "--no-bin-links",
    "--no-audit",
    "--no-fund",
    "--allow-file=all",
];

/// Unpacks Node if needed, runs npm, checks for links, renames into place and writes the
/// receipt. The temp folder is removed when anything fails.
pub(super) async fn install<T: Timer + Send + Sync>(
    job: &Job<T>,
    node_choice: &NodeChoice,
    fetched: &Fetched,
    run: &Run,
) -> Result<Installed, TryError> {
    let config = &job.config;
    let agent = &job.ticket.agent;
    let snapshot = &config.snapshot;
    let platform = config.platform;
    let missing = || TryError::retry("the snapshot does not know this agent", config);
    let entry = snapshot.agent(agent).ok_or_else(missing)?;
    let pins = snapshot.pins(agent).ok_or_else(missing)?;
    let hash = snapshot.install_hash(agent, platform).ok_or_else(missing)?;
    let (program, cli, version) = match node_choice {
        NodeChoice::User(found) => (
            found.program.clone(),
            found.npm_cli.clone(),
            found.version.clone(),
        ),
        NodeChoice::Runtime { dir, fetch } => {
            if *fetch {
                let archive = snapshot.node_archive(platform);
                let (Some(path), Some(archive)) = (&fetched.node_archive, archive) else {
                    return Err(TryError::retry("Node's archive is missing", config));
                };
                node::install_runtime(path, snapshot.node(), archive, dir, platform.os)
                    .map_err(|err| TryError::extract(&err, config))?;
            }
            (
                node::runtime_program(dir, platform.os),
                node::runtime_npm_cli(dir, platform.os),
                format!("v{}", snapshot.node().version()),
            )
        }
    };
    let source = match node_choice {
        NodeChoice::User(_) => NodeSource::User,
        NodeChoice::Runtime { .. } => NodeSource::Pinned,
    };
    tracing::info!(%agent, "agent install: {}", node_line(source, &program, &version));
    if !job.emit(InstallState::Installing, false) {
        return Err(TryError::cancelled());
    }
    let io = |what: &'static str| io_error(what, config);
    let cache = npm_cache(&config.data_dir);
    // Made through no link below the data folder, as `install/` is below, so the removals in
    // them never reach a folder a link points at.
    write::create_private_dirs_under(&config.data_dir, &cache)
        .map_err(io("making the npm cache"))?;
    let npmrc = config.data_dir.join("agents").join("npmrc");
    write::write_atomic(&npmrc, b"").map_err(io("writing npmrc"))?;
    let global = config.data_dir.join("agents").join("npmrc-global");
    write::write_atomic(&global, b"").map_err(io("writing npmrc-global"))?;
    let root = install_root(&config.data_dir, agent);
    write::create_private_dirs_under(&config.data_dir, &root)
        .map_err(io("making the install folder"))?;
    let tmp = root.join(format!(".tmp-{hash}-{}", run.attempts));
    write::remove_dir_all(&tmp).map_err(io("clearing the temp folder"))?;
    write::create_dir_all(&tmp).map_err(io("making the temp folder"))?;
    let npm = Npm {
        job,
        program: &program,
        cli: &cli,
        runtime: match node_choice {
            NodeChoice::Runtime { dir, .. } => Some(dir.as_path()),
            NodeChoice::User(_) => None,
        },
        common: common_flags(&cache, &npmrc, &global),
    };
    let done = async {
        let lock = pins
            .lock_with_tarballs(&fetched.tarballs, platform)
            .map_err(|err| TryError::stop(&err.to_string(), config))?;
        write::write_atomic(&tmp.join("package.json"), pins.package_json().as_bytes())
            .map_err(io("writing package.json"))?;
        write::write_atomic(&tmp.join("package-lock.json"), lock.as_bytes())
            .map_err(io("writing package-lock.json"))?;
        let mut args: Vec<OsString> = vec!["ci".into()];
        args.extend(CI_FLAGS.iter().map(OsString::from));
        npm.run("ci", args, &tmp).await?;
        if let Some(link) = first_link(&tmp) {
            let detail = format!("npm left a link in the install folder: {}", link.display());
            return Err(TryError::retry(&detail, config));
        }
        let folder = install_folder(&config.data_dir, agent, &hash);
        write::remove_dir_all(&folder).map_err(io("clearing the install folder"))?;
        write::rename(&tmp, &folder).map_err(io("renaming the install folder"))?;
        let receipt = Receipt::new(
            agent,
            pins.version(),
            &hash,
            &program,
            &version,
            platform,
            config.clock.now(),
        );
        write_receipt(&folder, &receipt).map_err(io("writing the receipt"))?;
        Ok(Installed {
            node: program.clone(),
            script: script_path(&folder, entry),
            folder,
            version: pins.version().to_owned(),
        })
    }
    .await;
    if done.is_err() {
        drop(write::remove_dir_all(&tmp));
    }
    done
}

/// Maps a file error of `what` to a try's error.
fn io_error<'a>(
    what: &'static str,
    config: &'a AgentsConfig,
) -> impl FnOnce(std::io::Error) -> TryError + 'a {
    move |err| TryError::io(what, &err, config)
}

/// Flags every npm run gets: the private cache, the empty npmrc files as user and global config,
/// no update check and no log files. Never a proxy or a CA file (see the module's comment).
fn common_flags(cache: &Path, npmrc: &Path, global: &Path) -> Vec<OsString> {
    vec![
        "--cache".into(),
        cache.as_os_str().to_owned(),
        "--userconfig".into(),
        npmrc.as_os_str().to_owned(),
        "--globalconfig".into(),
        global.as_os_str().to_owned(),
        "--update-notifier=false".into(),
        "--logs-max=0".into(),
    ]
}

/// The first symbolic link or junction under `dir`, not following any.
fn first_link(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                return Some(path);
            }
            if meta.is_dir() {
                stack.push(path);
            }
        }
    }
    None
}

/// One try's npm.
struct Npm<'a, T> {
    job: &'a Job<T>,
    program: &'a Path,
    cli: &'a Path,
    /// The downloaded runtime's folder when npm runs from it: removed when npm cannot start or
    /// cannot load its own modules, so the next try fetches Node again.
    runtime: Option<&'a Path>,
    common: Vec<OsString>,
}

impl<T> Npm<'_, T> {
    /// Removes the runtime npm ran from, if it did, and says so in `detail`.
    fn drop_runtime(&self, detail: &mut String) {
        if let Some(dir) = self.runtime {
            drop(write::remove_dir_all(dir));
            detail.push_str("; the downloaded Node is removed and will be fetched again");
        }
    }
}

impl<T: Timer + Send + Sync> Npm<'_, T> {
    /// Runs `node <npm-cli.js> <args> <common flags>` in `cwd`.
    async fn run(&self, what: &str, args: Vec<OsString>, cwd: &Path) -> Result<(), TryError> {
        let config = &self.job.config;
        let mut argv = vec![
            OsString::from(NODE_SINGLE_THREADED),
            self.cli.as_os_str().to_owned(),
        ];
        argv.extend(args);
        argv.extend(self.common.iter().cloned());
        let refused = |err: clauseo_process::Error| {
            let mut detail = format!("npm {what} could not start: {err}");
            self.drop_runtime(&mut detail);
            TryError::retry(&detail, config)
        };
        let spec = LaunchSpec::new(
            self.program.to_owned(),
            argv,
            BTreeMap::new(),
            cwd.to_owned(),
        )
        .map_err(refused)?;
        let Launched {
            stdin,
            mut stdout,
            stderr,
            mut handle,
            ..
        } = self.job.launcher.launch(&spec).map_err(refused)?;
        drop(stdin);
        let mut code_line: Option<String> = None;
        let mut module_missing = false;
        let ran = {
            let work = pin!(async {
                let read = async {
                    let mut chunk = [0_u8; 8192];
                    while let Ok(1..) = stdout.read(&mut chunk).await {}
                };
                let lines = stderr.lines(|line| {
                    let line = line.trim();
                    if code_line.is_none() && line.starts_with("npm error code ") {
                        code_line = Some(line.to_owned());
                    }
                    // Node's own report when npm-cli.js cannot load one of npm's files.
                    if line.starts_with("Error: Cannot find module")
                        || line.contains("MODULE_NOT_FOUND")
                    {
                        module_missing = true;
                    }
                });
                let ((), drained, report) = join3(read, lines, handle.exited()).await;
                drop(drained);
                report.code()
            });
            let cancelled = pin!(self.job.cancel().cancelled());
            let limit = pin!(self.job.timer.after(NPM_LIMIT));
            match select(work, select(cancelled, limit)).await {
                Either::Left((code, _)) => Ok(code),
                Either::Right((Either::Left(_), _)) => Err(None),
                Either::Right((Either::Right(_), _)) => Err(Some(())),
            }
        };
        drop(handle);
        match ran {
            Ok(Some(0)) => Ok(()),
            Ok(code) => {
                let code =
                    code.map_or_else(|| "a signal".to_owned(), |code| format!("code {code}"));
                let mut detail = format!("npm {what} exited with {code}");
                if let Some(line) = &code_line {
                    detail.push_str(": ");
                    detail.push_str(line);
                }
                if module_missing && code_line.is_none() {
                    self.drop_runtime(&mut detail);
                }
                let enospc = code_line.as_deref().is_some_and(|l| l.ends_with("ENOSPC"));
                Err(TryError::npm(&detail, enospc, config))
            }
            Err(None) => Err(TryError::cancelled()),
            Err(Some(())) => Err(TryError::retry(
                &format!("npm {what} did not finish in 10 minutes"),
                config,
            )),
        }
    }
}
