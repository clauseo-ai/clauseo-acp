//! The owner's side of the row: the set-up (recipe steps 1, 3 and 4 and the test account), the
//! helper's start, the run, the artifacts, the evidence, and a cleanup that always runs, in a
//! fixed order, each step checked and printed.

use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use clauseo_lowbox::{
    BrowserFolders, Namespace, NamespaceHandles, PackageName, Sid, grants, package_sid,
};

use super::acl::{self, DriveRoot, NullDevice};
use super::evidence::{self, Context};
use super::helper::Config;
use super::launch::{self, Helper};
use super::proxy::Proxy;
use super::station::Station;
use super::win::{self, Res};
use super::{account, loopback, write};

/// How long the owner waits for chrome's page request.
const PAGE_WAIT: Duration = Duration::from_secs(60);
/// How long it waits for the helper's facts after `inspect`.
const FACTS_WAIT: Duration = Duration::from_secs(60);
/// How long it waits for the helper to exit after the release.
const HELPER_EXIT_MS: u32 = 120_000;
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
struct User {
    /// Its name.
    name: String,
    /// Its SID text, once looked up.
    sid: Option<String>,
}

/// Everything the row made, each field set as soon as the thing exists, so the cleanup undoes
/// exactly what was done.
#[derive(Default)]
struct Run {
    /// The helper process.
    helper: Option<Helper>,
    /// The proxy.
    proxy: Option<Proxy>,
    /// The namespace's handles.
    namespace: Option<NamespaceHandles>,
    /// The drive root's saved DACL.
    drive_root: Option<DriveRoot>,
    /// `\Device\Null`'s saved DACL.
    null_device: Option<NullDevice>,
    /// The loopback list as it was.
    loopback: Option<Vec<Vec<u8>>>,
    /// The SID that holds the granted logon right.
    logon_right: Option<String>,
    /// The station and desktop.
    station: Option<Station>,
    /// The test account.
    user: Option<User>,
    /// The test root.
    root: Option<PathBuf>,
    /// Whether the cleanup ran.
    cleaned: bool,
}

/// The whole row: set up, prove, clean up, then fail with every problem found.
pub(crate) fn row() {
    let inputs = Inputs::from_env();
    let mut run = Run::default();
    let outcome = run.prove(&inputs);
    let cleanup = run.cleanup();
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
        println!("setup: test account {user}, package {}", package.as_str());

        let password = win::random_password();
        account::add(&user, &password)?;
        self.user = Some(User {
            name: user.clone(),
            sid: None,
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
        let namespace = self.namespace.insert(
            Namespace::build(&package, session, &browser).map_err(because("Namespace::build"))?,
        );
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

        let output = self.watch();
        self.finish(inputs, &folders, &station_name, &output)
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
    /// releases the page and waits for the helper. Returns the helper's output lines.
    fn watch(&mut self) -> Vec<String> {
        let (Some(helper), Some(proxy)) = (self.helper.as_mut(), self.proxy.as_ref()) else {
            return Vec::new();
        };
        let seen = proxy.wait_seen(PAGE_WAIT);
        println!(
            "run: the proxy {} the page request",
            if seen { "saw" } else { "never saw (60 s)" }
        );
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
                    println!("run: no inspection report within 60 s");
                    break;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        proxy.release();
        println!("run: page released");
        if !helper.wait(HELPER_EXIT_MS) {
            println!("run: the helper did not exit within 120 s");
        }
        while let Ok(line) = helper.next_line(DRAIN_WAIT) {
            output.push(line);
        }
        output
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
        let shot_bytes = std::fs::metadata(&shot).map_or(0, |m| m.len());
        write::create_dir_all(&inputs.artifacts)
            .map_err(|e| format!("making the artifacts folder: {e}"))?;
        if shot_bytes > 0 {
            write::copy(&shot, &inputs.artifacts.join("lowbox-shot.png"))
                .map_err(|e| format!("copying shot.png: {e}"))?;
        }
        let log = self.helper.as_ref().map(Helper::log).unwrap_or_default();
        let lines = evidence::parse(output);
        let context = Context {
            station: station_name.to_owned(),
            station_exists: self.station.as_ref().is_some_and(Station::exists),
            shot_bytes,
            proxy_seen: self.proxy.as_ref().map_or(0, Proxy::seen),
            helper_exit: self.helper.as_ref().and_then(Helper::exit_code),
        };
        let mut summary = evidence::summary(&lines, &context);
        if let Some(proxy) = self.proxy.as_ref() {
            let (refused, errors) = proxy.refused_and_errors();
            summary.push(format!(
                "evidence: proxy refused {refused:?}, errors {errors:?}"
            ));
        }
        let problems = evidence::check(&lines, &context);
        println!("--- the helper's log ---");
        for line in &log {
            println!("{line}");
        }
        println!("--- the evidence ---");
        for line in &summary {
            println!("{line}");
        }
        let mut file = log.join("\n");
        file.push_str("\n\n");
        file.push_str(&summary.join("\n"));
        write::write(&inputs.artifacts.join("lowbox-helper-log.txt"), file)
            .map_err(|e| format!("writing the helper's log: {e}"))?;
        Ok(problems)
    }

    /// Undoes what the row did, in the brief's order, printing each step; the failures.
    fn cleanup(&mut self) -> Vec<String> {
        let mut failures = Vec::new();
        let mut step = |name: &str, result: Res<String>| match result {
            Ok(done) => println!("cleanup: {name}: {done}"),
            Err(e) => {
                println!("cleanup: {name}: FAILED: {e}");
                failures.push(format!("{name}: {e}"));
            }
        };
        if let Some(helper) = self.helper.take() {
            step("kill leftovers", helper.kill());
        }
        if let Some(mut proxy) = self.proxy.take() {
            step("stop the proxy", Ok(proxy.stop()));
        }
        if let Some(namespace) = self.namespace.take() {
            drop(namespace);
            step(
                "delete the namespace handles",
                Ok("closed all six".to_owned()),
            );
        }
        if let Some(drive_root) = self.drive_root.take() {
            step("restore the drive root's DACL", drive_root.restore());
        }
        if let Some(null_device) = self.null_device.take() {
            step(r"restore \Device\Null's DACL", null_device.restore());
        }
        if let Some(original) = self.loopback.take() {
            step("restore the loopback list", loopback::restore(&original));
        }
        if let Some(sid) = self.logon_right.take() {
            step(
                "remove SeInteractiveLogonRight",
                account::remove_logon(&sid),
            );
        }
        if let Some(mut station) = self.station.take() {
            step("close the station and desktop", station.close());
        }
        if let Some(user) = self.user.take() {
            step("NetUserDel", account::delete(&user.name));
            if let Some(sid) = user.sid {
                step("DeleteProfileW", account::delete_profile(&sid));
            }
        }
        if let Some(root) = self.root.take() {
            let removed = write::remove_dir_all(&root)
                .map(|()| format!("deleted {}", root.display()))
                .map_err(|e| format!("{}: {e}", root.display()));
            step("delete the test root", removed);
        }
        self.cleaned = true;
        failures
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if !self.cleaned {
            println!("cleanup after a panic:");
            let failures = self.cleanup();
            println!("cleanup after a panic: {} step(s) failed", failures.len());
        }
    }
}
