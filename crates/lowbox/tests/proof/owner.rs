//! The owner's side of the row: the set-up (recipe steps 1, 3 and 4 and the test account), the
//! helper's start, the run, the artifacts and the evidence. The cleanup, which always runs, is in
//! `teardown.rs`.
//!
//! The row's own bounded waits: the page 180 s (the helper's first four CDP steps, 20 s each,
//! run inside it), the facts 60 s, the helper's exit 150 s (navigate, load and screenshot 20 s
//! each, `Browser.close` 10 s, chrome's exit 30 s, the exit codes 15 s, chrome's stderr 10 s:
//! 125 s), its last output 10 s, then in the cleanup the helper's kill 10 s, the account's
//! processes 30 s, the profile's delete 57 s and the root's delete 10 s: 507 s in all, under the
//! 600 s nextest gives this row (`.config/nextest.toml`).

use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use clauseo_lowbox::{
    BrowserFolders, Namespace, NamespaceHandles, PackageName, Sid, grants, package_sid,
};

use super::acl::{self, DriveRoot, NullDevice};
use super::evidence::{self, Context, PNG_SIGNATURE, redact};
use super::helper::{Config, STDERR_LOG};
use super::launch::{self, Helper};
use super::proxy::Proxy;
use super::station::Station;
use super::win::{self, Res};
use super::{account, loopback, teardown, write};

/// How long the owner waits for chrome's page request.
const PAGE_WAIT: Duration = Duration::from_secs(180);
/// How long it waits for the helper's facts after `inspect`.
const FACTS_WAIT: Duration = Duration::from_secs(60);
/// How long it waits for the helper to exit after the release.
const HELPER_EXIT_MS: u32 = 150_000;
/// How long it waits for the helper's last output after its exit.
const DRAIN_WAIT: Duration = Duration::from_secs(10);

/// The row's inputs from CI.
struct Inputs {
    /// chrome-headless-shell.exe (x64).
    shell: PathBuf,
    /// The folder CI uploads afterwards.
    artifacts: PathBuf,
    /// `%SystemDrive%`, such as `C:`.
    system_drive: String,
}

impl Inputs {
    /// Reads them; a missing one fails the row (it never skips).
    fn from_env() -> Inputs {
        let var = |name: &str| {
            std::env::var(name).unwrap_or_else(|_| {
                panic!("{name} is not set: the proof row needs it (arm64-browser-proof.yml)")
            })
        };
        assert_eq!(
            var("CLAUSEO_ELEVATED_TESTS"),
            "1",
            "CLAUSEO_ELEVATED_TESTS must be 1: the row runs elevated"
        );
        let shell = PathBuf::from(var("CLAUSEO_TEST_CHROME_SHELL"));
        assert!(
            shell.is_file(),
            "CLAUSEO_TEST_CHROME_SHELL is not a file: {}",
            shell.display()
        );
        let artifacts = PathBuf::from(var("CLAUSEO_TEST_ARTIFACTS"));
        let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        Inputs {
            shell,
            artifacts,
            system_drive,
        }
    }
}

/// The test account.
pub(crate) struct User {
    /// Its name.
    pub(crate) name: String,
    /// Its SID text, once looked up.
    pub(crate) sid: Option<String>,
    /// `%SystemDrive%\Users\<name>`: its profile folder when `ProfileImagePath` is gone.
    pub(crate) profile_fallback: PathBuf,
}

/// The namespace's handles and what opens it again by name.
pub(crate) struct Built {
    /// The handles `Namespace::build` returned.
    pub(crate) handles: NamespaceHandles,
    /// Its package.
    pub(crate) package: PackageName,
    /// Its session.
    pub(crate) session: u32,
}

/// Everything the row made, each field set as soon as the thing exists, so the cleanup undoes
/// exactly what was done.
#[derive(Default)]
pub(crate) struct Run {
    /// The helper process.
    pub(crate) helper: Option<Helper>,
    /// The proxy.
    pub(crate) proxy: Option<Proxy>,
    /// The namespace.
    pub(crate) namespace: Option<Built>,
    /// The drive root's saved DACL.
    pub(crate) drive_root: Option<DriveRoot>,
    /// `\Device\Null`'s saved DACL.
    pub(crate) null_device: Option<NullDevice>,
    /// The loopback list as it was.
    pub(crate) loopback: Option<Vec<Vec<u8>>>,
    /// The SID that holds the granted logon right.
    pub(crate) logon_right: Option<String>,
    /// The station and desktop.
    pub(crate) station: Option<Station>,
    /// The test account.
    pub(crate) user: Option<User>,
    /// The test root.
    pub(crate) root: Option<PathBuf>,
    /// Whether the cleanup ran.
    pub(crate) cleaned: bool,
}

/// The whole row: set up, prove, clean up, then fail with every problem found.
pub(crate) fn row() {
    let inputs = Inputs::from_env();
    let mut run = Run::default();
    let outcome = run.prove(&inputs);
    let cleanup = teardown::cleanup(&mut run);
    let mut problems = match outcome {
        Ok(problems) => problems,
        Err(e) => vec![format!("the row stopped: {e}")],
    };
    problems.extend(cleanup.into_iter().map(|f| format!("cleanup: {f}")));
    assert!(
        problems.is_empty(),
        "lowbox_headless_shell_row failed:\n- {}",
        problems.join("\n- ")
    );
    println!("lowbox_headless_shell_row: every assertion held and the cleanup is complete");
}

/// `e` as a sentence about `what`.
fn because(what: &str) -> impl Fn(clauseo_lowbox::Error) -> String + '_ {
    move |e| format!("{what}: {e}")
}

/// A path as text.
fn text(path: &Path) -> String {
    path.display().to_string()
}

impl Run {
    /// The set-up and the run; `Ok` holds the assertions that failed.
    fn prove(&mut self, inputs: &Inputs) -> Res<Vec<String>> {
        let tag = win::random_hex(6);
        let user = format!("ClauseoLowbox{tag}");
        let package = PackageName::new(&format!("Clauseo.Browser.Proof{tag}"))
            .map_err(because("the package name"))?;
        let package_text = package_sid(&package).as_str().to_owned();
        println!("setup: test account {user} and a fresh package (its name is not printed)");

        let password = win::random_password();
        account::add(&user, &password)?;
        self.user = Some(User {
            name: user.clone(),
            sid: None,
            profile_fallback: PathBuf::from(format!(r"{}\Users\{user}", inputs.system_drive)),
        });
        let user_sid = account::lookup_sid(&user)?;
        if let Some(made) = self.user.as_mut() {
            made.sid = Some(user_sid.clone());
        }
        account::grant_logon(&user_sid)?;
        self.logon_right = Some(user_sid.clone());
        println!("setup: made the account and granted SeInteractiveLogonRight for the run");

        let root = PathBuf::from(format!(
            r"{}\clauseo-lowbox-proof-{tag}",
            inputs.system_drive
        ));
        acl::create_root(&root, &user_sid)?;
        self.root = Some(root.clone());
        let (shell, helper_exe, folders) = Self::lay_out(&root, inputs, &package)?;
        println!("setup: test root {} laid out and granted", root.display());

        let drive_root = format!(r"{}\", inputs.system_drive);
        let added = self
            .drive_root
            .insert(DriveRoot::add(&drive_root, &package_text)?);
        added.check(&package_text)?;
        let added = self.null_device.insert(NullDevice::add(&package_text)?);
        added.check(&package_text)?;
        println!(r"setup: package ACEs added on {drive_root} and \Device\Null (re-read)");

        let original = loopback::current()?;
        self.loopback = Some(original.clone());
        let mut exempt = original;
        exempt.push(package_sid(&package).to_bytes());
        loopback::set(&exempt)?;
        println!("setup: loopback exemption added");

        let owner = win::token_user_sid()?;
        let station = self.station.insert(Station::create(
            &format!("ClauseoLowbox-{tag}"),
            &owner,
            &user_sid,
            &package_text,
        )?);
        let (station_name, desktop) = (station.name().to_owned(), station.desktop_path());
        println!("setup: private station {station_name} and its desktop Default");

        let port = self
            .proxy
            .insert(Proxy::start().map_err(|e| format!("the proxy: {e}"))?)
            .port();
        let browser = Sid::parse(&user_sid).map_err(because("the account's SID"))?;
        let session = Namespace::current_session().map_err(because("the session"))?;
        assert_ne!(
            session, 0,
            "the row runs in session 0, which has no \\Sessions\\0\\BaseNamedObjects for the \
             namespace: run it from a logged-on session"
        );
        let handles =
            Namespace::build(&package, session, &browser).map_err(because("Namespace::build"))?;
        let namespace = &self
            .namespace
            .insert(Built {
                handles,
                package: package.clone(),
                session,
            })
            .handles;
        println!("setup: namespace built in session {session}; proxy on 127.0.0.1:{port}");

        let helper = self
            .helper
            .insert(launch::start(&user, &password, &helper_exe, &desktop)?);
        drop(password);
        let values = namespace
            .duplicate_into(helper.process())
            .map_err(because("duplicate_into"))?;
        let config = Config {
            handles: values
                .iter()
                .map(|v| format!("{v:#x}"))
                .collect::<Vec<String>>()
                .join(","),
            package: package.as_str().to_owned(),
            shell: text(&shell),
            profile: text(&folders.profile),
            staging: text(&folders.staging),
            tmp: text(&folders.tmp),
            port,
        };
        helper.send(&config.line())?;
        println!(
            "run: helper pid {} started as {user}; handles copied",
            helper.pid()
        );

        match self.watch() {
            Ok(output) => self.finish(inputs, &folders, &station_name, &output),
            Err(e) => {
                if let Err(log) = self.save_logs(inputs, &folders, &[]) {
                    println!("run: {log}");
                }
                Err(e)
            }
        }
    }

    /// Makes the test root's folders, applies the package's grants, then copies the shell and
    /// the helper in (so their files inherit). Returns the shell's path, the helper's path and
    /// the folders.
    fn lay_out(
        root: &Path,
        inputs: &Inputs,
        package: &PackageName,
    ) -> Res<(PathBuf, PathBuf, BrowserFolders)> {
        let folders = BrowserFolders {
            profile: root.join("profile"),
            staging: root.join("staging"),
            tmp: root.join("tmp"),
            shell: root.join("shell"),
        };
        let pins = root.join("pins");
        let helper_dir = root.join("helper");
        for dir in [
            &folders.shell,
            &folders.profile,
            &folders.staging,
            &folders.tmp,
            &pins,
            &helper_dir,
        ] {
            acl::mkdir(dir)?;
        }
        for grant in grants(package, &folders, &pins).map_err(because("grants"))? {
            acl::grant(&grant.path, grant.rights.mask(), grant.sid.as_str())?;
        }
        let source = inputs.shell.parent().ok_or("the shell has no folder")?;
        let files = write::copy_tree(source, &folders.shell)
            .map_err(|e| format!("copying the shell's folder: {e}"))?;
        let name = inputs
            .shell
            .file_name()
            .ok_or("the shell has no file name")?;
        let helper_exe = helper_dir.join("clauseo-lowbox-proof-helper.exe");
        let me = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        write::copy(&me, &helper_exe).map_err(|e| format!("copying the test binary: {e}"))?;
        println!("setup: copied {files} shell files and the test binary as the helper");
        Ok((folders.shell.join(name), helper_exe, folders))
    }

    /// The run itself: waits for the page request, asks for the inspection, collects the facts,
    /// releases the page and waits for the helper. Returns the helper's output lines; without a
    /// page request it fails without asking for the inspection.
    fn watch(&mut self) -> Res<Vec<String>> {
        let (Some(helper), Some(proxy)) = (self.helper.as_mut(), self.proxy.as_ref()) else {
            return Err("the run had no helper or proxy".to_owned());
        };
        // One second at a time, so a helper that stopped (a failed CDP step) ends the wait.
        let mut seen = false;
        for _ in 0..PAGE_WAIT.as_secs() {
            seen = proxy.wait_seen(Duration::from_secs(1));
            if seen || helper.exit_code().is_some() {
                break;
            }
        }
        if !seen {
            return Err(format!(
                "the proxy never saw the page request (waited up to {} s; helper exit {:?}); no \
                 inspection asked (the helper's log is above)",
                PAGE_WAIT.as_secs(),
                helper.exit_code()
            ));
        }
        println!("run: the proxy saw the page request");
        if let Err(e) = helper.send("inspect") {
            println!("run: {e}");
        }
        let mut output = Vec::new();
        loop {
            match helper.next_line(FACTS_WAIT) {
                Ok(line) => {
                    let last = line.contains("|inspected ");
                    output.push(line);
                    if last {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    println!(
                        "run: no inspection report within {} s",
                        FACTS_WAIT.as_secs()
                    );
                    break;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        proxy.release();
        println!("run: page released");
        if !helper.wait(HELPER_EXIT_MS) {
            println!(
                "run: the helper did not exit within {} s",
                HELPER_EXIT_MS / 1000
            );
        }
        while let Ok(line) = helper.next_line(DRAIN_WAIT) {
            output.push(line);
        }
        Ok(output)
    }

    /// Copies the artifacts, prints the evidence and checks it.
    fn finish(
        &mut self,
        inputs: &Inputs,
        folders: &BrowserFolders,
        station_name: &str,
        output: &[String],
    ) -> Res<Vec<String>> {
        let shot = folders.staging.join("shot.png");
        let shot_data = std::fs::read(&shot).unwrap_or_default();
        let shot_bytes = u64::try_from(shot_data.len()).unwrap_or(u64::MAX);
        write::create_dir_all(&inputs.artifacts)
            .map_err(|e| format!("making the artifacts folder: {e}"))?;
        if shot_bytes > 0 {
            write::copy(&shot, &inputs.artifacts.join("lowbox-shot.png"))
                .map_err(|e| format!("copying shot.png: {e}"))?;
        }
        let lines = evidence::parse(output);
        let context = Context {
            station: station_name.to_owned(),
            shot_bytes,
            shot_png: shot_data.starts_with(&PNG_SIGNATURE),
            proxy_delivered: self.proxy.as_ref().map_or(0, Proxy::delivered),
            helper_exit: self.helper.as_ref().and_then(Helper::exit_code),
        };
        let mut summary = evidence::summary(&lines, &context);
        if let Some(proxy) = self.proxy.as_ref() {
            let (refused, errors) = proxy.refused_and_errors();
            summary.push(format!(
                "evidence: proxy page requests {}, refused {refused:?}, errors {errors:?}",
                proxy.seen()
            ));
        }
        let problems = evidence::check(&lines, &context);
        self.save_logs(inputs, folders, &summary)?;
        Ok(problems)
    }

    /// Prints the helper's log and `summary`, writes both to the artifacts and copies chrome's
    /// standard error there, each with the package SIDs cut out (`redact`).
    fn save_logs(&self, inputs: &Inputs, folders: &BrowserFolders, summary: &[String]) -> Res<()> {
        let log: Vec<String> = self
            .helper
            .as_ref()
            .map(Helper::log)
            .unwrap_or_default()
            .iter()
            .map(|line| redact(line))
            .collect();
        let summary: Vec<String> = summary.iter().map(|line| redact(line)).collect();
        println!("--- the helper's log ---");
        for line in &log {
            println!("{line}");
        }
        println!("--- the evidence ---");
        for line in &summary {
            println!("{line}");
        }
        write::create_dir_all(&inputs.artifacts)
            .map_err(|e| format!("making the artifacts folder: {e}"))?;
        let mut file = log.join("\n");
        file.push_str("\n\n");
        file.push_str(&summary.join("\n"));
        write::write(&inputs.artifacts.join("lowbox-helper-log.txt"), file)
            .map_err(|e| format!("writing the helper's log: {e}"))?;
        match std::fs::read(folders.staging.join(STDERR_LOG)) {
            Ok(bytes) => {
                let text = redact(&String::from_utf8_lossy(&bytes));
                write::write(&inputs.artifacts.join("lowbox-chrome-stderr.log"), &text)
                    .map_err(|e| format!("writing chrome's standard error: {e}"))?;
                println!("artifacts: chrome's standard error, {} bytes", bytes.len());
            }
            Err(e) => println!("artifacts: no chrome standard error log ({e})"),
        }
        Ok(())
    }
}
