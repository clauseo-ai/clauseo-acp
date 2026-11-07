//! `prepare` and `cleanup`.
//!
//! Prepare makes the harness root with `bin\` (the runner, this exe, and node and codex when
//! given), `logs\`, matters `A` and `B` (each a matter and a state folder), and a data folder
//! unless one is given or the machine's pool record names one; runs the set-up when no pool is
//! set up for that data folder; picks two pool users; grants each `(OI)(CI)M` on its own matter
//! and state folders with the Low label; and records all of it in the manifest.
//!
//! Cleanup deletes only what the manifest lists, each by its full path, and runs the set-up's
//! `--remove` only when prepare ran the set-up. When that fails the pool's record still names
//! the data folder, so the data folder (and the root holding it) stay, and it says so.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use clauseo_sandbox::windows::pool::pool_number;
use clauseo_sandbox::windows::{sandbox_data_dir, sandbox_pool_sids};

use super::cli::Opts;
use super::launch::{deadline, run_command, text};
use super::manifest::{Manifest, Pool};
use super::sys::{copy_file, make_dir, output, remove_file, remove_tree, say, write_file};
use super::win;

/// The copied mock model server.
const MOCK: &str = include_str!("mock-responses.mjs");
/// Its file name in the harness root.
pub(crate) const MOCK_FILE: &str = "mock-responses.mjs";

/// Runs prepare; `Err` with why it stopped. What was made so far is in the manifest.
pub(crate) fn prepare(opts: &Opts) -> Result<Manifest, String> {
    let root = &opts.root;
    if Manifest::path_in(root).exists() {
        return Err(format!(
            "{} exists: already prepared; run cleanup first",
            Manifest::path_in(root).display()
        ));
    }
    let mut m = Manifest {
        root: root.clone(),
        ..Manifest::default()
    };
    if !root.exists() {
        make(&mut m, root)?;
    }
    m.save().map_err(|e| format!("writing the manifest: {e}"))?;
    say(&format!("prepare: root {}", root.display()));
    users_read_check(root);
    for dir in [m.bin(), m.logs()] {
        make(&mut m, &dir)?;
    }
    copy_programs(opts, &mut m)?;
    m.save().map_err(|e| format!("writing the manifest: {e}"))?;
    find_pool(opts, &mut m)?;
    m.save().map_err(|e| format!("writing the manifest: {e}"))?;
    let (a, b) = m
        .pool_pair()
        .map(|(a, b)| (a.clone(), b.clone()))
        .ok_or("no pool pair")?;
    for (which, pool) in [("A", &a), ("B", &b)] {
        for dir in [m.matter(which), m.state(which)] {
            make(&mut m, &dir)?;
            m.save().map_err(|e| format!("writing the manifest: {e}"))?;
            icacls(&dir, &["/grant", &format!("*{}:(OI)(CI)M", pool.sid)])?;
            icacls(&dir, &["/setintegritylevel", "(OI)(CI)L"])?;
            say(&format!(
                "prepare: matter {which} {} granted to {} ({}) with (OI)(CI)M and the Low label",
                dir.display(),
                pool.name,
                pool.sid
            ));
        }
    }
    let mock = root.join(MOCK_FILE);
    write_file(&mock, MOCK.as_bytes()).map_err(|e| format!("writing the mock: {e}"))?;
    m.files.push(mock);
    m.save().map_err(|e| format!("writing the manifest: {e}"))?;
    say(&format!(
        "prepare: done; manifest {}",
        Manifest::path_in(root).display()
    ));
    Ok(m)
}

/// Makes `dir` and records it. One already there is refused, never recorded, so cleanup removes
/// only folders the harness made.
fn make(m: &mut Manifest, dir: &Path) -> Result<(), String> {
    if dir.exists() {
        return Err(format!(
            "{} exists already: pick another --root",
            dir.display()
        ));
    }
    make_dir(dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
    m.dirs.push(dir.to_path_buf());
    Ok(())
}

/// Says whether `root` holds an inherited `BUILTIN\Users:(RX)` entry, read from icacls's text
/// (English account names): the restricted token reaches `bin\` through `Users`.
fn users_read_check(root: &Path) {
    let mut command = Command::new("icacls");
    command.arg(root);
    let Ok(out) = output(&mut command) else {
        say("prepare: icacls couldn't run; the Users check is skipped");
        return;
    };
    let listing = String::from_utf8_lossy(&out.stdout).into_owned();
    let inherited = listing
        .lines()
        .any(|line| line.contains(r"BUILTIN\Users:(I)") && line.contains("RX)"));
    if inherited {
        say("prepare: the root inherits BUILTIN\\Users:(RX) (icacls): the agent can read bin");
    } else {
        say("prepare: WARNING no inherited BUILTIN\\Users:(RX) on the root; icacls says:");
        for line in listing.lines().filter(|l| !l.trim().is_empty()) {
            say(&format!("prepare:   {}", line.trim()));
        }
    }
}

/// `exe` next to this program, or one folder up (cargo puts examples in `examples\`).
fn beside_me(exe: &str) -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let dir = me.parent()?;
    [dir.join(exe), dir.parent()?.join(exe)]
        .into_iter()
        .find(|path| path.is_file())
}

/// Copies the runner, this exe, and node and codex when given, into bin.
fn copy_programs(opts: &Opts, m: &mut Manifest) -> Result<(), String> {
    let runner = match &opts.runner {
        Some(path) => path.clone(),
        None => beside_me(clauseo_sandbox::windows::runner::RUNNER_EXE)
            .ok_or("clauseo-sandbox-run.exe not found beside this exe: pass --runner")?,
    };
    let me = std::env::current_exe().map_err(|e| format!("this exe's path: {e}"))?;
    for (from, to) in [(runner, m.runner_exe()), (me, m.harness_exe())] {
        copy_file(&from, &to).map_err(|e| format!("copying {}: {e}", from.display()))?;
        say(&format!(
            "prepare: copied {} to {}",
            from.display(),
            to.display()
        ));
    }
    if let Some(node) = &opts.node {
        let dir = m.bin().join("node");
        make_dir(&dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
        let to = dir.join("node.exe");
        copy_file(node, &to).map_err(|e| format!("copying {}: {e}", node.display()))?;
        say(&format!(
            "prepare: copied {} to {}",
            node.display(),
            to.display()
        ));
        m.node = Some(to);
    }
    if let Some(codex) = &opts.codex {
        super::codex::copy_codex(codex, m)?;
    }
    Ok(())
}

/// Copies the folder `from` into `to`, recursively (no links followed).
pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    make_dir(to).map_err(|e| format!("making {}: {e}", to.display()))?;
    let entries =
        std::fs::read_dir(from).map_err(|e| format!("reading {}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("reading {}: {e}", from.display()))?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            copy_file(&entry.path(), &target)
                .map_err(|e| format!("copying {}: {e}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Finds the data folder and two pool users, running the set-up when no pool is set up for the
/// data folder.
fn find_pool(opts: &Opts, m: &mut Manifest) -> Result<(), String> {
    let recorded = sandbox_data_dir().map_err(|e| e.to_string())?;
    let pool = sandbox_pool_sids().map_err(|e| e.to_string())?;
    m.data_dir = match (&opts.data_dir, &recorded) {
        (Some(given), _) => given.clone(),
        (None, Some(recorded)) if !pool.is_empty() => recorded.clone(),
        (None, _) => m.root.join("data"),
    };
    if !m.data_dir.exists() {
        let data = m.data_dir.clone();
        make(m, &data)?;
    }
    let same = recorded
        .as_ref()
        .is_some_and(|r| text(r).eq_ignore_ascii_case(&text(&m.data_dir)));
    let mut sids = pool;
    if sids.len() < 2 || !same {
        say(&format!(
            "prepare: no pool is set up for {} (record: {:?}, {} pool users): running the set-up",
            m.data_dir.display(),
            recorded,
            sids.len()
        ));
        run_set_up(opts, m)?;
        sids = sandbox_pool_sids().map_err(|e| e.to_string())?;
    } else {
        say(&format!(
            "prepare: using the pool set up for {} ({} users)",
            m.data_dir.display(),
            sids.len()
        ));
    }
    for sid in sids.iter().take(2) {
        let name = win::account_name(sid.as_str())
            .map_err(|e| format!("LookupAccountSidW({sid}): {e}"))?;
        if pool_number(&name).is_none() {
            return Err(format!("{sid} is {name}, not a pool user's name"));
        }
        m.pools.push(Pool {
            sid: sid.as_str().to_owned(),
            name,
        });
    }
    match m.pool_pair() {
        Some((a, b)) => {
            say(&format!("prepare: matter A runs as {} ({})", a.name, a.sid));
            say(&format!("prepare: matter B runs as {} ({})", b.name, b.sid));
            Ok(())
        }
        None => Err(format!("the pool has {} users; two are needed", sids.len())),
    }
}

/// `clauseo-sandbox-setup --set-up --parent <this pid> --data-dir <data>`, with this user made
/// the data folder's owner first (an elevated token makes `Administrators` the owner).
fn run_set_up(opts: &Opts, m: &mut Manifest) -> Result<(), String> {
    let exe = match &opts.setup {
        Some(path) => path.clone(),
        None => beside_me("clauseo-sandbox-setup.exe")
            .ok_or("clauseo-sandbox-setup.exe not found beside this exe: pass --setup")?,
    };
    let me = win::own_token()
        .map_err(|e| format!("this token: {e}"))?
        .user;
    icacls(&m.data_dir, &["/setowner", &format!("*{me}")])?;
    let mut command = Command::new(&exe);
    command
        .args([
            "--set-up",
            "--parent",
            &std::process::id().to_string(),
            "--data-dir",
        ])
        .arg(&m.data_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    m.setup_exe = Some(exe);
    // Recorded before it runs: a set-up that half ran still gets cleanup's --remove.
    m.setup_ran = true;
    m.save().map_err(|e| format!("writing the manifest: {e}"))?;
    let running = run_command(&mut command)?;
    let done = running.finish(deadline(opts.row_timeout.max(Duration::from_secs(300))))?;
    for line in done.stdout.iter().chain(&done.stderr) {
        say(&format!("prepare:   setup: {line}"));
    }
    say(&format!(
        "prepare: set-up exited {} after {} ms",
        done.code, done.wall_ms
    ));
    let result = m.data_dir.join("sandbox").join("setup-result.json");
    if let Ok(text) = std::fs::read_to_string(&result) {
        say(&format!("prepare:   setup-result.json: {}", text.trim()));
    }
    if done.code != 0 {
        return Err(format!(
            "the set-up exited {} (it needs an elevated token; on the VM set up through the app)",
            done.code
        ));
    }
    Ok(())
}

/// `icacls <dir> <args>`; `Err` with its output when it fails.
fn icacls(dir: &Path, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new("icacls");
    command.arg(dir).args(args);
    let out = output(&mut command).map_err(|e| format!("icacls couldn't run: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "icacls {} {} failed: {}{}",
        dir.display(),
        args.join(" "),
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

/// Runs cleanup for the root `root`; `false` when something stayed that should have gone.
pub(crate) fn cleanup(root: &Path) -> bool {
    let mut m = match Manifest::load(root) {
        Ok(Some(m)) => m,
        Ok(None) => {
            say(&format!(
                "cleanup: no manifest in {}: nothing to clean",
                root.display()
            ));
            return true;
        }
        Err(e) => {
            say(&format!("cleanup: the manifest can't be read: {e}"));
            return false;
        }
    };
    let mut clean = true;
    for name in std::mem::take(&mut m.atoms) {
        clean &= delete_atoms(&name);
    }
    let mut keep: Vec<PathBuf> = Vec::new();
    if m.setup_ran {
        if remove_pool(&m) {
            m.setup_ran = false;
        } else {
            say(&format!(
                "cleanup: kept {}: the pool's record still names it",
                m.data_dir.display()
            ));
            keep.push(m.data_dir.clone());
        }
    }
    for file in m.files.iter().rev() {
        match remove_file(file) {
            Ok(()) => say(&format!("cleanup: removed {}", file.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                say(&format!("cleanup: couldn't remove {}: {e}", file.display()));
                clean = false;
            }
        }
    }
    let manifest = Manifest::path_in(root);
    let mut left = Vec::new();
    for dir in m.dirs.iter().rev() {
        // A folder holding a kept one stays (only the root holds the data folder).
        if keep.iter().any(|kept| kept.starts_with(dir)) {
            left.push(dir.clone());
            continue;
        }
        if dir == root {
            drop(remove_file(&manifest));
        }
        match remove_tree(dir) {
            Ok(()) => say(&format!(
                "cleanup: removed {} (its grants and label with it)",
                dir.display()
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                say(&format!("cleanup: couldn't remove {}: {e}", dir.display()));
                left.push(dir.clone());
                clean = false;
            }
        }
    }
    if manifest.exists() {
        m.files.clear();
        left.reverse();
        m.dirs = left;
        match m.save() {
            Ok(()) => say(&format!(
                "cleanup: the manifest keeps what stayed: {}",
                manifest.display()
            )),
            Err(e) => say(&format!("cleanup: the manifest couldn't be rewritten: {e}")),
        }
    }
    clean
}

/// Deletes every reference to the global atom `name` (the atoms row's, left by a row that
/// stopped); whether none is left.
pub(crate) fn delete_atoms(name: &str) -> bool {
    for _ in 0..64 {
        let atom = super::ui::find_atom(name);
        if atom == 0 {
            say(&format!("cleanup: no global atom {name} left"));
            return true;
        }
        if !super::ui::delete_atom(atom) {
            break;
        }
    }
    say(&format!(
        "cleanup: the global atom {name} couldn't be deleted"
    ));
    false
}

/// `clauseo-sandbox-setup --remove`; whether it exited 0.
fn remove_pool(m: &Manifest) -> bool {
    let Some(exe) = &m.setup_exe else {
        say("cleanup: the manifest names no set-up exe; --remove skipped");
        return false;
    };
    let mut command = Command::new(exe);
    command
        .arg("--remove")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let done = run_command(&mut command).and_then(|r| r.finish(deadline(Duration::from_secs(300))));
    match done {
        Ok(done) if done.code == 0 => {
            say("cleanup: clauseo-sandbox-setup --remove exited 0");
            true
        }
        Ok(done) => {
            for line in done.stdout.iter().chain(&done.stderr) {
                say(&format!("cleanup:   setup: {line}"));
            }
            say(&format!(
                "cleanup: clauseo-sandbox-setup --remove exited {} (PR4j builds the removal)",
                done.code
            ));
            false
        }
        Err(e) => {
            say(&format!("cleanup: clauseo-sandbox-setup --remove: {e}"));
            false
        }
    }
}
