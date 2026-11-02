//! A try's install step: Node's runtime unpacked when it was downloaded, then
//! `node npm-cli.js cache add` in batches and `npm ci --offline` in `install/.tmp-<hash>-<n>`,
//! a walk that refuses any link, the rename to `<hash>` and the receipt, written last.
//!
//! Every npm run starts through the launcher as `node <npm-cli.js> …`, with `NODE_OPTIONS`
//! empty, the private cache, an empty `agents/npmrc` as its user config, and the proxy and CA
//! file when set. Its stderr pump and its stdout are driven to their end while its exit is
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
use crate::node;
use crate::receipt::{Receipt, install_folder, install_root, script_path, write_receipt};
use crate::state::{InstallState, Installed};
use crate::write;

/// The most tarballs one `npm cache add` gets.
const BATCH: usize = 40;
/// The longest one npm run may take.
const NPM_LIMIT: Duration = Duration::from_mins(10);
/// The flags `npm ci` runs with, besides the common ones.
const CI_FLAGS: [&str; 5] = [
    "--offline",
    "--ignore-scripts",
    "--no-bin-links",
    "--no-audit",
    "--no-fund",
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
    if !job.emit(InstallState::Installing, false) {
        return Err(TryError::cancelled());
    }
    let io = |what: &'static str| io_error(what, config);
    let cache = npm_cache(&config.data_dir);
    write::create_dir_all(&cache).map_err(io("making the npm cache"))?;
    let npmrc = config.data_dir.join("agents").join("npmrc");
    write::write_atomic(&npmrc, b"").map_err(io("writing npmrc"))?;
    let root = install_root(&config.data_dir, agent);
    let tmp = root.join(format!(".tmp-{hash}-{}", run.attempts));
    write::remove_dir_all(&tmp).map_err(io("clearing the temp folder"))?;
    write::create_dir_all(&tmp).map_err(io("making the temp folder"))?;
    let npm = Npm {
        job,
        program: &program,
        cli: &cli,
        common: common_flags(config, job, &cache, &npmrc),
        env: env(config, job),
    };
    let done = async {
        write::write_atomic(&tmp.join("package.json"), pins.package_json().as_bytes())
            .map_err(io("writing package.json"))?;
        write::write_atomic(&tmp.join("package-lock.json"), pins.lock().as_bytes())
            .map_err(io("writing package-lock.json"))?;
        for batch in fetched.tarballs.chunks(BATCH) {
            let mut args: Vec<OsString> = vec!["cache".into(), "add".into()];
            args.extend(batch.iter().map(|path| path.as_os_str().to_owned()));
            npm.run("cache add", args, &tmp).await?;
        }
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

/// Flags every npm run gets: the private cache, the empty user config, no update check, no
/// log files, and the proxy and CA file when set.
fn common_flags<T>(
    config: &AgentsConfig,
    job: &Job<T>,
    cache: &Path,
    npmrc: &Path,
) -> Vec<OsString> {
    let mut flags: Vec<OsString> = vec![
        "--cache".into(),
        cache.as_os_str().to_owned(),
        "--userconfig".into(),
        npmrc.as_os_str().to_owned(),
        "--update-notifier=false".into(),
        "--logs-max=0".into(),
    ];
    let proxy = match (config.install.proxy.as_deref(), job.system_proxy.as_deref()) {
        (Some(proxy), _) => Some(proxy.to_owned()),
        (None, Some(system)) if system.contains("://") => Some(system.to_owned()),
        (None, Some(system)) => Some(format!("http://{system}")),
        (None, None) => None,
    };
    if let Some(proxy) = proxy {
        flags.extend([
            "--proxy".into(),
            OsString::from(&proxy),
            "--https-proxy".into(),
            OsString::from(proxy),
        ]);
    }
    if let Some(ca) = config.install.ca_certs.get(&job.ticket.agent) {
        flags.extend(["--cafile".into(), ca.as_os_str().to_owned()]);
    }
    flags
}

/// `NODE_OPTIONS` empty, and `NODE_EXTRA_CA_CERTS` when the agent has a CA file.
fn env<T>(config: &AgentsConfig, job: &Job<T>) -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::from([(OsString::from("NODE_OPTIONS"), OsString::new())]);
    if let Some(ca) = config.install.ca_certs.get(&job.ticket.agent) {
        env.insert("NODE_EXTRA_CA_CERTS".into(), ca.as_os_str().to_owned());
    }
    env
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
    common: Vec<OsString>,
    env: BTreeMap<OsString, OsString>,
}

impl<T: Timer + Send + Sync> Npm<'_, T> {
    /// Runs `node <npm-cli.js> <args> <common flags>` in `cwd`.
    async fn run(&self, what: &str, args: Vec<OsString>, cwd: &Path) -> Result<(), TryError> {
        let config = &self.job.config;
        let mut argv = vec![self.cli.as_os_str().to_owned()];
        argv.extend(args);
        argv.extend(self.common.iter().cloned());
        let refused = |err: clauseo_process::Error| {
            TryError::retry(&format!("npm {what} could not start: {err}"), config)
        };
        let spec = LaunchSpec::new(
            self.program.to_owned(),
            argv,
            self.env.clone(),
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
