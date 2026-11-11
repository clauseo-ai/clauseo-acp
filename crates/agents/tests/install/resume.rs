//! An install cut short and resumed: three interruptions of `npm ci` (a killed npm, a pause, a
//! quit) leave the disk as one install leaves it, with no npm cache growing across them; and an
//! `npm ci` that meets `EBUSY` runs again in place, with no new download and no try counted.
//! A rename refused with Windows' `Access is denied` (injected) is retried in place too; one
//! that never works fails as `SetupFailed`, not as a failed download.
//! Each interrupted `npm ci` is a fake that writes into the npm cache as npm does, then sleeps.
#![expect(
    clippy::disallowed_methods,
    reason = "real processes: the tests poll for the fake npm's pid and walk the data folder"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use clauseo_agents::testing::trace;
use clauseo_agents::{Agents, FailureKind, InstallState};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions, kill};

#[cfg(unix)]
use crate::support::TREE_POLL;
use crate::support::{A_PACKAGES, Events, Fixture, agent_a};

/// The fake npm that is interrupted: writes two 64 KiB entries into the npm cache its first
/// argument names (as npm's `_cacache` does, under names no other run uses), then its pid as
/// `<n>.pid` in the folder its second argument names, then sleeps.
const HANGING_NPM: &str = "\
const fs = require('fs'), path = require('path');
const [cache, dir, n] = process.argv.slice(1);
const put = (file) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, Buffer.alloc(65536));
};
put(path.join(cache, '_cacache', 'tmp', 'tmp-' + process.pid));
put(path.join(cache, '_cacache', 'content-v2', 'sha512', 'entry-' + process.pid));
const file = path.join(dir, n + '.pid');
fs.writeFileSync(file + '.tmp', String(process.pid));
fs.renameSync(file + '.tmp', file);
setInterval(() => {}, 1000);
";

/// The fake npm whose tree outlives it a moment: a child in its group (its pid as `<n>.child`),
/// then its pid as `<n>.pid` in the folder its first argument names, then sleeps. How soon the
/// OS reaps the two after the group's kill varies, so on Unix the tests hold a process of their
/// own in the group as well ([`Holder`]), to make the job's wait poll for certain.
const HANGING_TREE_NPM: &str = "\
const fs = require('fs'), path = require('path');
const [dir, n] = process.argv.slice(1);
const child = require('child_process').spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
fs.writeFileSync(path.join(dir, n + '.child'), String(child.pid));
const file = path.join(dir, n + '.pid');
fs.writeFileSync(file + '.tmp', String(process.pid));
fs.renameSync(file + '.tmp', file);
setInterval(() => {}, 1000);
";

/// The fake npm that meets a held file: npm's own error-code line, then exit 1.
const BUSY_NPM: &str = "process.stderr.write('npm error code EBUSY\\n'); process.exit(1);";

/// How long the fake npm's pid may take to appear.
pub(crate) const DEADLINE: Duration = Duration::from_secs(30);
/// The in-place retry's first wait, and the job's first back-off.
const IN_PLACE_FIRST: Duration = Duration::from_millis(250);
pub(crate) const BACKOFF_FIRST: Duration = Duration::from_secs(2);

/// What one `npm ci` launch runs.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Fake {
    Hang,
    HangTree,
    Busy,
}

/// The passthrough launcher, with the first `npm ci` launches replaced by `plan`'s fakes in
/// order; the ones after run the real npm.
struct Planned {
    inner: PassthroughLauncher,
    plan: Vec<Fake>,
    ci_runs: Arc<AtomicUsize>,
    pids: PathBuf,
}

impl Launcher for Planned {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        if !spec.args().iter().any(|arg| arg == "ci") {
            return self.inner.launch(spec);
        }
        let n = self.ci_runs.fetch_add(1, Ordering::SeqCst);
        let Some(fake) = self.plan.get(n).copied() else {
            trace(format_args!("launch: the real npm ci, run {n}"));
            return self.inner.launch(spec);
        };
        trace(format_args!(
            "launch: the fake npm ({fake:?}) in place of npm ci, run {n}"
        ));
        let args = spec.args();
        let cache = args
            .iter()
            .position(|arg| arg == "--cache")
            .and_then(|at| args.get(at + 1))
            .expect("npm runs with --cache")
            .clone();
        let mut argv: Vec<OsString> = vec!["--single-threaded".into(), "-e".into()];
        match fake {
            Fake::Hang => argv.extend([
                HANGING_NPM.into(),
                cache,
                self.pids.as_os_str().to_owned(),
                n.to_string().into(),
            ]),
            Fake::HangTree => argv.extend([
                HANGING_TREE_NPM.into(),
                self.pids.as_os_str().to_owned(),
                n.to_string().into(),
            ]),
            Fake::Busy => argv.push(BUSY_NPM.into()),
        }
        let env = spec.env().clone();
        let fake = LaunchSpec::new(spec.program().to_owned(), argv, env, spec.cwd().to_owned())?;
        self.inner.launch(&fake)
    }
}

/// Starts `Agents` on `fx` with `plan` (its count of `npm ci` runs kept in `ci_runs` across
/// restarts) and reads the first states.
pub(crate) fn start(fx: &Fixture, plan: &[Fake], ci_runs: &Arc<AtomicUsize>) -> (Agents, Events) {
    let launcher: Arc<dyn Launcher> = Arc::new(Planned {
        inner: PassthroughLauncher::new(SpawnOptions::new()),
        plan: plan.to_vec(),
        ci_runs: Arc::clone(ci_runs),
        pids: pid_dir(fx),
    });
    let (agents, events) = Agents::start(fx.config.clone(), launcher, fx.timer.clone());
    let events = Events::new(events);
    events.first_states();
    (agents, events)
}

/// The folder the fake npm writes its pids to, outside the data folder's measure.
fn pid_dir(fx: &Fixture) -> PathBuf {
    let dir = PathBuf::from(format!("{}-pids", fx.data.path().display()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The pid of the `n`th `npm ci` run's fake, once it has written into the cache.
fn fake_pid(fx: &Fixture, n: usize) -> u32 {
    let file = pid_dir(fx).join(format!("{n}.pid"));
    let start = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(&file) {
            return text.trim().parse().expect("a pid");
        }
        assert!(start.elapsed() < DEADLINE, "no {} in time", file.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Every file under `dir` by its path below `dir`, with its size; nothing when it is missing.
pub(crate) fn files_under(dir: &Path) -> BTreeMap<String, u64> {
    let mut found = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                stack.push(path);
            } else {
                let below = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                found.insert(below, meta.len());
            }
        }
    }
    found
}

/// The npm cache's bytes.
fn cache_bytes(fx: &Fixture) -> u64 {
    files_under(&fx.data.path().join("agents").join("npm-cache"))
        .values()
        .sum()
}

/// The data folder after one install of agent A with nothing in its way. The label is as long
/// as the interrupted test's, so the absolute paths npm writes into its files are as long too.
fn one_install() -> BTreeMap<String, u64> {
    let fx = Fixture::new("resume-uninterrupted");
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert!(agents.shutdown(Duration::from_secs(10)));
    files_under(fx.data.path())
}

#[test]
fn three_interruptions_then_ready_leave_one_install_on_disk() {
    let fx = Fixture::new("resume-three-stopped");
    let ci_runs = Arc::new(AtomicUsize::new(0));
    let plan = [Fake::Hang; 3];
    let (agents, events) = start(&fx, &plan, &ci_runs);
    agents.install(&agent_a());

    // 1: the fake npm is killed; the job's back-off, then its next try.
    let pid = fake_pid(&fx, 0);
    let one_run = cache_bytes(&fx);
    assert_eq!(one_run, 2 * 65536, "the fake wrote its two cache entries");
    kill(pid);
    fx.timer.fire_next(BACKOFF_FIRST);
    fake_pid(&fx, 1);
    assert_eq!(
        cache_bytes(&fx),
        one_run,
        "the next try starts with an empty cache"
    );

    // 2: a pause, then the user's install again.
    agents.pause(&agent_a());
    events.until(&agent_a(), |state| *state == InstallState::Paused);
    agents.install(&agent_a());
    fake_pid(&fx, 2);
    assert_eq!(
        cache_bytes(&fx),
        one_run,
        "the resumed job starts with an empty cache"
    );

    // 3: a quit while the fake sleeps, then a new start.
    fake_pid(&fx, 2);
    assert!(
        agents.shutdown(Duration::from_secs(10)),
        "the quit ended the job"
    );
    drop(agents);
    drop(events);
    let (agents, events) = start(&fx, &plan, &ci_runs);
    assert_eq!(cache_bytes(&fx), 0, "the start empties what the quit left");
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert!(agents.shutdown(Duration::from_secs(10)));
    assert_eq!(
        ci_runs.load(Ordering::SeqCst),
        4,
        "three fakes, then the real npm"
    );

    assert_eq!(cache_bytes(&fx), 0, "the npm cache is empty after Ready");
    for name in A_PACKAGES {
        assert_eq!(fx.requests_for(name).len(), 1, "{name}: one download");
    }
    let left = files_under(fx.data.path());
    let once = one_install();
    assert_eq!(left, once, "the disk holds what one install leaves");
    let total: u64 = left.values().sum();
    assert_eq!(total, once.values().sum::<u64>());
}

#[test]
fn an_npm_ci_that_meets_ebusy_runs_again_in_place_with_one_download() {
    let fx = Fixture::new("resume-busy");
    let ci_runs = Arc::new(AtomicUsize::new(0));
    let (agents, events) = start(&fx, &[Fake::Busy], &ci_runs);
    agents.install(&agent_a());
    let armed = fx.timer.fire_first_of(&[IN_PLACE_FIRST, BACKOFF_FIRST]);
    assert_eq!(
        armed, IN_PLACE_FIRST,
        "EBUSY waits 250 ms in place, not the job's back-off"
    );
    let (seen, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(
        ci_runs.load(Ordering::SeqCst),
        2,
        "the fake, then the real npm"
    );
    assert_eq!(agents.install_tries(&agent_a()), Some(1), "no try counted");
    assert_eq!(
        fx.timer.install_waits(),
        Vec::<Duration>::new(),
        "no back-off"
    );
    for name in A_PACKAGES {
        assert_eq!(fx.requests_for(name).len(), 1, "{name}: one download");
    }
    let installing = seen
        .iter()
        .filter(|(_, state)| *state == InstallState::Installing)
        .count();
    assert_eq!(installing, 1, "one try, its files fetched once: {seen:?}");
}

/// A rename fault answering `error()` for the first `times` renames (every one with `None`),
/// counting every rename.
pub(crate) fn rename_fault(
    times: Option<usize>,
    error: fn() -> std::io::Error,
) -> (Arc<clauseo_agents::RenameFault>, Arc<AtomicUsize>) {
    let renames = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&renames);
    let fault = move || {
        let n = counted.fetch_add(1, Ordering::SeqCst);
        times.is_none_or(|times| n < times).then(error)
    };
    (Arc::new(fault), renames)
}

#[test]
fn a_rename_denied_four_times_is_retried_in_place_with_one_try_and_one_download() {
    let mut fx = Fixture::new("resume-rename-held");
    let (fault, renames) = rename_fault(Some(4), || std::io::Error::from_raw_os_error(5));
    fx.config.install.rename_fault = Some(fault);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let waits = [
        Duration::from_millis(250),
        Duration::from_millis(500),
        Duration::from_secs(1),
        Duration::from_secs(2),
    ];
    for want in waits {
        let armed = fx.timer.fire_first_of(&[want, Duration::from_secs(8)]);
        assert_eq!(armed, want, "the rename's in-place waits, in order");
    }
    let (seen, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(
        renames.load(Ordering::SeqCst),
        5,
        "four refused, the fifth works"
    );
    assert_eq!(agents.install_tries(&agent_a()), Some(1), "one try");
    let installing = seen
        .iter()
        .filter(|(_, state)| *state == InstallState::Installing)
        .count();
    assert_eq!(installing, 1, "no try started over: {seen:?}");
    for name in A_PACKAGES {
        assert_eq!(fx.requests_for(name).len(), 1, "{name}: one download");
    }
}

#[test]
fn a_rename_that_never_works_fails_as_setup_failed_not_as_a_download() {
    let mut fx = Fixture::new("resume-rename-fails");
    let (fault, renames) = rename_fault(None, || std::io::Error::other("the rename failed"));
    fx.config.install.rename_fault = Some(fault);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.timer.fire_next(BACKOFF_FIRST);
    fx.timer.fire_next(Duration::from_secs(8));
    let (_, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("{last:?}");
    };
    assert_eq!(failure.kind, FailureKind::SetupFailed);
    assert_eq!(failure.kind.code(), "agent.install_setup_failed");
    assert!(
        failure
            .log
            .lines()
            .iter()
            .any(|line| line.contains("renaming the install folder")),
        "{:?}",
        failure.log
    );
    assert_eq!(
        renames.load(Ordering::SeqCst),
        3,
        "one rename per try, not retried in place"
    );
    for name in A_PACKAGES {
        assert_eq!(fx.requests_for(name).len(), 1, "{name}: one download");
    }
}

/// Node's archive that downloads and checks but will not unpack (a gzip of no tar): Node is
/// unpacked right after its download, before the agent's packages (so another install waits for
/// Node alone), and the failure is the set-up's, not the download's; `Installing` never went out.
#[cfg(unix)]
#[test]
fn node_s_unpack_runs_before_the_packages_and_its_failure_is_a_setup_failure() {
    use std::io::Write as _;

    use crate::support::{BASE, node_entry, node_path, snapshot};

    let mut fx = Fixture::new("resume-bad-unpack");
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(b"this is not a tar archive").unwrap();
    let archive = gz.finish().unwrap();
    let entry = node_entry(Some(&archive));
    let path = node_path(&entry);
    fx.registry.route(&path, archive);
    fx.config.snapshot = snapshot(BASE, entry, 10_000);
    fx.config.node_search = clauseo_agents::NodeSearch::none();
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.timer.fire_next(BACKOFF_FIRST);
    fx.timer.fire_next(Duration::from_secs(8));
    let (seen, last) = events.until_end(&agent_a());
    assert!(
        !crate::support::states_of(&seen, &agent_a()).contains(&InstallState::Installing),
        "{seen:?}"
    );
    for name in A_PACKAGES {
        assert_eq!(
            fx.requests_for(name),
            [],
            "{name}: no package before Node is in place"
        );
    }
    let InstallState::Failed(failure) = last else {
        panic!("{last:?}");
    };
    assert_eq!(failure.kind, FailureKind::SetupFailed, "{:?}", failure.log);
    let fetched = fx
        .registry
        .requests()
        .into_iter()
        .filter(|request| request.path == path)
        .count();
    assert_eq!(fetched, 1, "the archive downloaded once");
}

/// The `n`th fake npm's tree: its pid and its child's, once both are written.
fn tree_pids(fx: &Fixture, n: usize) -> [u32; 2] {
    let npm = fake_pid(fx, n);
    let child = std::fs::read_to_string(pid_dir(fx).join(format!("{n}.child"))).unwrap();
    [npm, child.trim().parse().expect("a pid")]
}

/// Whether the temp folder npm ran in is still in the install root.
fn temp_folder_there(fx: &Fixture) -> bool {
    std::fs::read_dir(fx.install_root(&agent_a()))
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp-"))
}

/// Waits until the job has removed the temp folder npm ran in, then checks that npm's whole
/// tree (`pids`) had ended by then: the folder never goes while the tree may hold a file in it.
fn folder_went_after_the_tree(fx: &Fixture, pids: [u32; 2]) {
    let start = Instant::now();
    while temp_folder_there(fx) {
        assert!(
            start.elapsed() < DEADLINE,
            "the temp folder was not removed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    for pid in pids {
        let running = clauseo_process::running(pid);
        if running {
            kill(pid);
        }
        assert!(
            !running,
            "npm's tree ran on after its temp folder went: {pid}"
        );
    }
}

/// The polls for npm's tree armed so far.
#[cfg(unix)]
fn tree_polls(fx: &Fixture) -> usize {
    fx.timer
        .armed()
        .into_iter()
        .filter(|wait| *wait == TREE_POLL)
        .count()
}

/// A process of the test's own in npm's process group, which the test reaps only when it lets
/// go: killed with the group, it stays in it as the test's zombie, and the job's wait (an empty
/// group) sees the tree as running however fast the OS reaps npm's own processes.
#[cfg(unix)]
struct Holder(std::process::Child);

#[cfg(unix)]
impl Holder {
    /// Joins the group npm (`npm`, its leader) runs in.
    fn join(npm: u32) -> Holder {
        use std::os::unix::process::CommandExt;
        let group = i32::try_from(npm).expect("a pid");
        let child = std::process::Command::new("sleep")
            .arg("600")
            .process_group(group)
            .spawn()
            .expect("a process in npm's group");
        Holder(child)
    }

    /// Waits until the job has armed a second poll for npm's tree since `before` (so its first
    /// poll found the tree running and went on), checks the temp folder is still there while
    /// the tree is, then reaps the holder.
    fn let_go_after_polls(mut self, fx: &Fixture, before: usize, what: &str) {
        let start = Instant::now();
        while tree_polls(fx) < before + 2 {
            assert!(
                start.elapsed() < DEADLINE,
                "{what} waited for npm's tree: {:?}",
                fx.timer.armed()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            temp_folder_there(fx),
            "the temp folder went while the tree ran"
        );
        self.0.wait().expect("the holder reaped");
    }
}

#[cfg(unix)]
impl Drop for Holder {
    /// Ends and reaps it if a failed assert left it; once reaped, both do nothing.
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

/// A pause while `npm ci` runs waits for npm's whole tree (polled, up to 10 s) before the temp
/// folder is removed, rather than ending the wait at once as the pause does elsewhere.
#[test]
fn a_pause_mid_npm_ci_waits_for_npm_s_tree() {
    let fx = Fixture::new("resume-pause-tree");
    let ci_runs = Arc::new(AtomicUsize::new(0));
    let (agents, events) = start(&fx, &[Fake::HangTree], &ci_runs);
    agents.install(&agent_a());
    let pids = tree_pids(&fx, 0);
    // Unix only: Windows' TerminateJobObject has usually emptied the job by the first check,
    // and the test cannot hold a process of its own in npm's job.
    #[cfg(unix)]
    let (holder, before) = (Holder::join(pids[0]), tree_polls(&fx));
    agents.pause(&agent_a());
    #[cfg(unix)]
    holder.let_go_after_polls(&fx, before, "the pause");
    events.until(&agent_a(), |state| *state == InstallState::Paused);
    folder_went_after_the_tree(&fx, pids);
    assert!(agents.shutdown(Duration::from_secs(10)), "the job ended");
}

/// `npm ci` running out of its 10 minutes waits for npm's tree too, before the job's retry.
#[test]
fn an_npm_ci_past_its_limit_waits_for_npm_s_tree() {
    let fx = Fixture::new("resume-limit-tree");
    let ci_runs = Arc::new(AtomicUsize::new(0));
    let (agents, _events) = start(&fx, &[Fake::HangTree], &ci_runs);
    agents.install(&agent_a());
    let pids = tree_pids(&fx, 0);
    // Unix only, as in the pause's test.
    #[cfg(unix)]
    let (holder, before) = (Holder::join(pids[0]), tree_polls(&fx));
    fx.timer.fire_next(Duration::from_mins(10));
    #[cfg(unix)]
    holder.let_go_after_polls(&fx, before, "the limit");
    folder_went_after_the_tree(&fx, pids);
    let start = Instant::now();
    while !fx.timer.install_waits().contains(&BACKOFF_FIRST) {
        assert!(start.elapsed() < DEADLINE, "no back-off after the limit");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(agents.shutdown(Duration::from_secs(10)));
}
