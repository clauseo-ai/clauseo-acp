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
//!
//! A sharing or lock error is retried in place ([`in_place`]): `npm ci` exiting with
//! `EBUSY`, `EPERM` or `EACCES`, and on Windows the rename meeting
//! `ERROR_SHARING_VIOLATION`, `ERROR_LOCK_VIOLATION` or `ERROR_ACCESS_DENIED` (an antivirus
//! scanning the files npm has just written holds them open for a moment). `npm ci` runs again
//! after 250 ms, 500 ms, 1 s and 2 s, the rename after those and 4 s and 8 s more, keeping the
//! temp folder and the checked tarballs; only then does the try fail and the job's own retry,
//! which starts the folder over, take it. Before either runs again, and before the rename,
//! npm's whole process tree (its job object on Windows, its group on Unix) has ended, so no
//! process of ours holds a file in the folder.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Timer, TreeWatch};
use clauseo_protocol::AgentId;
use futures::AsyncReadExt as _;
use futures::future::{Either, join3, select};

use super::files::{Fetched, npm_cache};
use super::{Job, NodeChoice, Run, TryError};
use crate::config::AgentsConfig;
use crate::node::{self, NodeSource, node_line};
use crate::platform::Os;
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
/// The waits before each in-place retry of `npm ci` after a sharing or lock error.
const NPM_WAITS: [Duration; 4] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];
/// The waits before each in-place retry of the rename after a sharing or lock error: 15.75 s
/// in all, since an antivirus may scan the thousands of fresh files for several seconds (the
/// Windows VM with Defender on met `Access is denied` on three tries in a row).
const RENAME_WAITS: [Duration; 6] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];
/// How often, and how many times, a finished npm's process tree is checked for its end
/// before the folder it ran in is used again: 10 s in all.
const TREE_POLL: Duration = Duration::from_millis(50);
const TREE_POLLS: u32 = 200;
/// npm's error codes for a file another process holds: retried in place.
const NPM_BUSY: [&str; 3] = ["EBUSY", "EPERM", "EACCES"];
/// Windows' raw errors for a file another process holds open or locked, or one being deleted
/// or scanned: `ERROR_ACCESS_DENIED`, `ERROR_SHARING_VIOLATION`, `ERROR_LOCK_VIOLATION`.
const WINDOWS_BUSY: [i32; 3] = [5, 32, 33];
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
    // As soon as every file is fetched and checked, before Node's unpack (thousands of files,
    // seconds under an antivirus): the set-up has started.
    if !job.emit(InstallState::Installing, false) {
        return Err(TryError::cancelled());
    }
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
        // `npm ci` empties `node_modules` itself, so a rerun in the same folder starts clean.
        in_place(
            "npm ci",
            agent,
            &NPM_WAITS,
            async |pause| job.wait(pause).await,
            async || npm.run("ci", args.clone(), &tmp).await,
            |failed: &NpmFailed| failed.busy().map(str::to_owned),
        )
        .await
        .map_err(|failed| failed.err)?;
        if let Some(link) = first_link(&tmp) {
            let detail = format!("npm left a link in the install folder: {}", link.display());
            return Err(TryError::retry(&detail, config));
        }
        let folder = install_folder(&config.data_dir, agent, &hash);
        write::remove_dir_all(&folder).map_err(io("clearing the install folder"))?;
        rename_held(job, &tmp, &folder)
            .await
            .map_err(io("renaming the install folder"))?;
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

/// Runs `step`, and again after each of `waits` while it fails with an error `busy`
/// names a sharing or lock cause for; any other error, the last retry's, or a pause during a
/// wait (`wait` answering `false`) ends it with that error. Each retry is logged with fixed
/// words, naming `what`, the cause and the wait.
pub(super) async fn in_place<R, E>(
    what: &str,
    agent: &AgentId,
    waits: &[Duration],
    wait: impl AsyncFn(Duration) -> bool,
    mut step: impl AsyncFnMut() -> Result<R, E>,
    busy: impl Fn(&E) -> Option<String>,
) -> Result<R, E> {
    let mut waits = waits.iter();
    loop {
        let err = match step().await {
            Ok(done) => return Ok(done),
            Err(err) => err,
        };
        let Some(cause) = busy(&err) else {
            return Err(err);
        };
        let Some(&pause) = waits.next() else {
            return Err(err);
        };
        tracing::warn!(
            %agent,
            step = what,
            cause,
            wait_ms = u64::try_from(pause.as_millis()).unwrap_or(u64::MAX),
            "agent install: a sharing or lock error; retrying the step in place"
        );
        if !wait(pause).await {
            return Err(err);
        }
    }
}

/// Renames `tmp` to `folder`, retried in place after [`RENAME_WAITS`] while another process
/// holds a file in it (Windows' 5, 32 or 33: a folder walker, an indexer, an antivirus).
async fn rename_held<T: Timer + Send + Sync>(
    job: &Job<T>,
    tmp: &Path,
    folder: &Path,
) -> std::io::Result<()> {
    let config = &job.config;
    let os = rename_os(config);
    in_place(
        "the rename",
        &job.ticket.agent,
        &RENAME_WAITS,
        async |pause| job.wait(pause).await,
        async || rename_into_place(config, tmp, folder),
        |err: &std::io::Error| windows_busy(err, os),
    )
    .await
}

/// Renames the finished temp folder to its `<hash>` folder; under `test-support`, a
/// [`crate::config::InstallConfig::rename_fault`] error comes first when it gives one.
fn rename_into_place(config: &AgentsConfig, tmp: &Path, folder: &Path) -> std::io::Result<()> {
    match rename_fault(config) {
        Some(err) => Err(err),
        None => write::rename(tmp, folder),
    }
}

/// A test's error for the next rename.
#[cfg(feature = "test-support")]
fn rename_fault(config: &AgentsConfig) -> Option<std::io::Error> {
    config
        .install
        .rename_fault
        .as_ref()
        .and_then(|fault| fault())
}

/// No test, no fault.
#[cfg(not(feature = "test-support"))]
fn rename_fault(_config: &AgentsConfig) -> Option<std::io::Error> {
    None
}

/// The OS whose rules judge a rename's error: the platform's, or Windows' under a test's
/// [`crate::config::InstallConfig::rename_fault`].
fn rename_os(config: &AgentsConfig) -> Os {
    #[cfg(feature = "test-support")]
    if config.install.rename_fault.is_some() {
        return Os::Win32;
    }
    config.platform.os
}

/// The cause when `err` on `os` is Windows' sharing, lock or access error, which a scan holding
/// the file causes: `None` on any other OS, where those raw numbers mean other things.
pub(super) fn windows_busy(err: &std::io::Error, os: Os) -> Option<String> {
    let code = err
        .raw_os_error()
        .filter(|code| WINDOWS_BUSY.contains(code))?;
    (os == Os::Win32).then(|| format!("Windows error {code}"))
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
    /// Waits, up to 10 s, until npm's whole tree has ended (its handle has sent the kill),
    /// so nothing of ours still holds a file in the folder npm ran in. A pause ends the wait.
    async fn tree_ended(&self, tree: &TreeWatch) {
        if tree.gone() {
            return;
        }
        let agent = &self.job.ticket.agent;
        tracing::info!(%agent, "agent install: waiting for npm's processes to end");
        for _ in 0..TREE_POLLS {
            if !self.job.wait(TREE_POLL).await || tree.gone() {
                return;
            }
        }
        tracing::warn!(%agent, "agent install: npm's processes had not ended after 10 s");
    }

    /// Runs `node <npm-cli.js> <args> <common flags>` in `cwd`.
    async fn run(&self, what: &str, args: Vec<OsString>, cwd: &Path) -> Result<(), NpmFailed> {
        self.run_once(what, args, cwd)
            .await
            .map_err(|(err, code_line)| NpmFailed {
                err,
                code: code_line.and_then(|line| npm_code(&line).map(str::to_owned)),
            })
    }

    /// [`Npm::run`], failing with npm's `npm error code X` line when it printed one.
    async fn run_once(
        &self,
        what: &str,
        args: Vec<OsString>,
        cwd: &Path,
    ) -> Result<(), (TryError, Option<String>)> {
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
        .map_err(|err| (refused(err), None))?;
        let Launched {
            stdin,
            mut stdout,
            stderr,
            mut handle,
            ..
        } = self
            .job
            .launcher
            .launch(&spec)
            .map_err(|err| (refused(err), None))?;
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
        let tree = handle.tree_watch();
        drop(handle);
        if ran.is_ok() {
            self.tree_ended(&tree).await;
        }
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
                Err((TryError::npm(&detail, enospc, config), code_line))
            }
            Err(None) => Err((TryError::cancelled(), None)),
            Err(Some(())) => Err((
                TryError::retry(&format!("npm {what} did not finish in 10 minutes"), config),
                None,
            )),
        }
    }
}

/// An npm run that failed: the try's error, and npm's error code when it printed one.
struct NpmFailed {
    err: TryError,
    code: Option<String>,
}

impl NpmFailed {
    /// npm's code when it is one of a file another process holds ([`NPM_BUSY`]).
    fn busy(&self) -> Option<&str> {
        self.code.as_deref().filter(|code| NPM_BUSY.contains(code))
    }
}

/// The code of npm's `npm error code X` line.
pub(super) fn npm_code(line: &str) -> Option<&str> {
    let code = line.strip_prefix("npm error code ")?.trim();
    (!code.is_empty() && !code.contains(char::is_whitespace)).then_some(code)
}

#[cfg(test)]
mod tests;
