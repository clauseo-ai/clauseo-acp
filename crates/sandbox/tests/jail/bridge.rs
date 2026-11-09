//! The agent's jail with a browser bridge (decision 0038, "The browser profile"), stage E's
//! T1–T4, T7 and T9: the agent cannot run, read, copy or map the browser binaries, while the
//! guard runs and reaches its socket. Every refusal is the OS's: no browser process starts
//! (the stand-in binaries are copies of `echo`, so one that ran would print its arguments).

use std::path::{Path, PathBuf};

use clauseo_sandbox::macos::INSTALLED_BROWSERS;
use clauseo_sandbox::{BrowserBridge, SeatbeltLauncher};

use crate::probe;
use crate::support::{Output, World, launcher, run, sh};

/// The arguments each browser binary is started with.
const FLAGS: &str = "--no-sandbox --headless";

/// Reaches the Unix socket `$ARGV[0]` with Perl: `CONNECTED`, or dies naming the error.
const CONNECT_UNIX: &str = r#"use Socket; socket(S, PF_UNIX, SOCK_STREAM, 0) or die "socket: $!\n"; connect(S, pack_sockaddr_un($ARGV[0])) or die "connect: $!\n"; print "CONNECTED\n""#;

/// Binds a Unix socket at `$ARGV[0]` with Perl: `BOUND`, or dies naming the error.
const BIND_UNIX: &str = r#"use Socket; socket(S, PF_UNIX, SOCK_STREAM, 0) or die "socket: $!\n"; bind(S, pack_sockaddr_un($ARGV[0])) or die "bind: $!\n"; print "BOUND\n""#;

/// The real Chrome, when this Mac has one.
const CHROME: &str = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const CHROME_FRAMEWORK: &str = concat!(
    "/Applications/Google Chrome.app/Contents/Frameworks/",
    "Google Chrome Framework.framework/Google Chrome Framework"
);

/// The jail tests' world plus stand-ins: the app's `Contents/Helpers` with agent-browser, the
/// downloaded headless shell in `<data>/browsers`, a guard outside both, a bundle of each
/// installed browser (and of one other app) in the home's `Applications`, and a run folder.
struct Bridged {
    w: World,
}

impl Bridged {
    fn new() -> Bridged {
        let w = World::new();
        // Copies of a system binary are killed outside the system's folders, so the programs
        // are scripts that print their arguments; the code to map is a copied system dylib.
        let echo = r#"echo "$@""#;
        w.script(
            "app/Clauseo.app/Contents/Helpers/clauseo-agent-browser",
            echo,
        );
        w.script("data/browsers/chs/chrome-headless-shell", echo);
        w.script("guard/clauseo-browser-guard", echo);
        for name in INSTALLED_BROWSERS.iter().chain(&["Other"]) {
            w.script(
                &format!("home/Applications/{name}.app/Contents/MacOS/{name}"),
                echo,
            );
        }
        w.dir("app/Clauseo.app/Contents/Frameworks");
        w.dir("home/Applications/Google Chrome.app/Contents/Frameworks");
        w.dir("data/browsers/chs/lib");
        for rel in [
            "app/Clauseo.app/Contents/Helpers/libhelper.dylib",
            "data/browsers/chs/libvulkan.dylib",
            "home/Applications/Google Chrome.app/Contents/Frameworks/Framework",
        ] {
            probe::dylib_to(&w.at(rel));
        }
        w.dir("data/browser-runs/r1/sock");
        w.file("data/browser-pins/pins.json", "{}\n");
        w.file("data/browser-runs/r1/staging/page.txt", "staged page\n");
        w.file("data/browser-runs/r2/staging/page.txt", "another run\n");
        Bridged { w }
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.w.at(rel)
    }

    fn bridge(&self) -> BrowserBridge {
        BrowserBridge {
            socket: self.at("data/browser-runs/r1/sock/guard.sock"),
            staging: self.at("data/browser-runs/r1/staging"),
            bundles: vec![
                self.at("app/Clauseo.app/Contents/Helpers"),
                self.at("data/browsers"),
            ],
            write_denied: vec![self.at("data/browser-pins")],
            ports: vec![],
        }
    }

    /// The jail with the app and the guard as runtime paths (read, run and map) and the home's
    /// `Applications` readable, so each refusal below is the bridge's, not the default's; with
    /// the bridge when `bridged`.
    fn launcher(&self, bridged: bool) -> SeatbeltLauncher {
        let mut b = self
            .w
            .builder()
            .runtime(self.at("app"))
            .runtime(self.at("guard"))
            .read(self.at("home/Applications"));
        if bridged {
            b = b.browser(self.bridge());
        }
        launcher(b.build().unwrap()).0
    }

    fn start(&self, l: &SeatbeltLauncher, program: &Path) -> Output {
        sh(
            l,
            &self.at("m1"),
            &format!(r#""$1" {FLAGS}"#),
            &[s(program)],
        )
    }

    fn map(&self, l: &SeatbeltLauncher, path: &Path) -> String {
        let probe = self.at("m1/probe");
        if !probe.exists() {
            probe::copy_to(&probe);
        }
        let env = probe::env("map", &[s(path)]);
        probe::answer(&run(l, s(&probe), &probe::ARGS, &self.at("m1"), &env).stdout)
    }
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Refused by the OS, and the binary never ran.
#[track_caller]
fn never_started(out: &Output) {
    out.assert_refused();
    assert!(
        !out.stdout.contains(FLAGS),
        "a browser binary ran: {out:#?}"
    );
}

#[test]
fn t1_no_browser_binary_starts() {
    let b = Bridged::new();
    let l = b.launcher(true);
    never_started(&b.start(&l, &b.at("data/browsers/chs/chrome-headless-shell")));
    for name in INSTALLED_BROWSERS {
        let fake = b.at(&format!(
            "home/Applications/{name}.app/Contents/MacOS/{name}"
        ));
        never_started(&b.start(&l, &fake));
    }
    if Path::new(CHROME).exists() {
        never_started(&b.start(&l, Path::new(CHROME)));
    } else {
        eprintln!("no {CHROME}: the stand-ins cover it");
    }
    // Another app in the same folder runs: the refusals are the bundle names'.
    let other = b.at("home/Applications/Other.app/Contents/MacOS/Other");
    assert!(b.start(&l, &other).assert_ok().stdout.contains(FLAGS));
    // Installed browsers are refused without a bridge too.
    let plain = b.launcher(false);
    let chrome = b.at("home/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    never_started(&b.start(&plain, &chrome));
}

#[test]
fn t2_browser_binaries_are_not_read_or_copied() {
    let b = Bridged::new();
    let l = b.launcher(true);
    let m1 = b.at("m1");
    for rel in [
        "app/Clauseo.app/Contents/Helpers/clauseo-agent-browser",
        "data/browsers/chs/chrome-headless-shell",
        "data/browsers/chs/libvulkan.dylib",
    ] {
        let from = b.at(rel);
        sh(&l, &m1, r#"cat "$1" > /dev/null"#, &[s(&from)]).assert_refused();
        sh(&l, &m1, r#"cp "$1" copied"#, &[s(&from)]).assert_refused();
        assert!(!m1.join("copied").exists(), "{rel} was copied");
    }
    // An installed browser keeps its read (0038), so a copy into the matter succeeds. Running
    // that copy from the matter is the residual T9 measures: no rule matches it there.
    let chrome = b.at("home/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    sh(&l, &m1, r#"cp "$1" copied"#, &[s(&chrome)]).assert_ok();
    assert!(m1.join("copied").exists());
}

#[test]
fn t3_browser_code_is_not_mapped() {
    let b = Bridged::new();
    let l = b.launcher(true);
    for rel in [
        "data/browsers/chs/libvulkan.dylib",
        "app/Clauseo.app/Contents/Helpers/libhelper.dylib",
        "home/Applications/Google Chrome.app/Contents/Frameworks/Framework",
    ] {
        let answer = b.map(&l, &b.at(rel));
        assert_ne!(answer, "map=ok", "{rel} was mapped");
    }
    if Path::new(CHROME_FRAMEWORK).exists() {
        assert_ne!(b.map(&l, Path::new(CHROME_FRAMEWORK)), "map=ok");
    }
}

#[test]
fn t4_agent_browser_is_refused_and_so_is_any_new_helper() {
    let b = Bridged::new();
    let l = b.launcher(true);
    let helpers = b.at("app/Clauseo.app/Contents/Helpers");
    never_started(&b.start(&l, &helpers.join("clauseo-agent-browser")));
    let m1 = b.at("m1");
    let from = helpers.join("clauseo-agent-browser");
    sh(&l, &m1, r#"cp "$1" copied"#, &[s(&from)]).assert_refused();
    // The tripwire: a helper nobody listed is refused by its folder's rule.
    let new = b.w.script(
        "app/Clauseo.app/Contents/Helpers/new-helper",
        r#"echo "$@""#,
    );
    never_started(&b.start(&l, &new));
}

/// The browser installs and their pins (`<data>/browsers`, `<data>/browser-pins`): nothing is
/// made, renamed or written there from the jail.
#[test]
fn browser_installs_and_pins_are_not_written() {
    let b = Bridged::new();
    let l = b.launcher(true);
    let m1 = b.at("m1");
    b.w.file("m1/mine.txt", "agent's\n");
    let shell = b.at("data/browsers/chs/chrome-headless-shell");
    for (dir, existing) in [
        ("data/browsers/chs", shell),
        ("data/browser-pins", b.at("data/browser-pins/pins.json")),
    ] {
        let dir = b.at(dir);
        sh(&l, &m1, r#": > "$1/new""#, &[s(&dir)]).assert_refused();
        sh(&l, &m1, r#"mv mine.txt "$1/moved""#, &[s(&dir)]).assert_refused();
        sh(&l, &m1, r#"mv "$1" "$1.old""#, &[s(&existing)]).assert_refused();
        sh(&l, &m1, r#"echo planted >> "$1""#, &[s(&existing)]).assert_refused();
        assert!(!dir.join("new").exists() && !dir.join("moved").exists() && existing.exists());
    }
    let pins = std::fs::read_to_string(b.at("data/browser-pins/pins.json")).unwrap();
    assert_eq!(pins, "{}\n");
}

/// T7, the part the profile decides: with a bridge the guard runs and reaches its socket, reads
/// its run's staging folder, and nothing else of the browser stack opens. The rest is stage E's.
#[test]
fn t7_the_guard_runs_and_reaches_only_its_own_run() {
    let b = Bridged::new();
    let socket = b.w.listen_unix("data/browser-runs/r1/sock/guard.sock");
    let other = b.w.listen_unix("data/browser-runs/r1/sock/other.sock");
    let l = b.launcher(true);
    let m1 = b.at("m1");
    let guard = b.at("guard/clauseo-browser-guard");
    assert!(b.start(&l, &guard).assert_ok().stdout.contains(FLAGS));
    let perl = |path: &Path| {
        run(
            &l,
            "/usr/bin/perl",
            &["-e", CONNECT_UNIX, s(path)],
            &m1,
            &[],
        )
    };
    assert!(perl(&socket).assert_ok().stdout.contains("CONNECTED"));
    perl(&other).assert_refused();
    // Nothing is bound, made or removed in sock/: the agent only connects.
    let sock = b.at("data/browser-runs/r1/sock");
    let bound = sock.join("agent.sock");
    run(&l, "/usr/bin/perl", &["-e", BIND_UNIX, s(&bound)], &m1, &[]).assert_refused();
    sh(&l, &m1, r#": > "$1""#, &[s(&sock.join("made"))]).assert_refused();
    sh(&l, &m1, r#"rm -f "$1""#, &[s(&socket)]).assert_refused();
    assert!(socket.exists() && !bound.exists() && !sock.join("made").exists());
    let staged = b.at("data/browser-runs/r1/staging/page.txt");
    let out = sh(&l, &m1, r#"cat "$1""#, &[s(&staged)]);
    assert_eq!(out.assert_ok().stdout, "staged page\n");
    sh(&l, &m1, r#"echo x >> "$1""#, &[s(&staged)]).assert_refused();
    let elsewhere = b.at("data/browser-runs/r2/staging/page.txt");
    sh(&l, &m1, r#"cat "$1""#, &[s(&elsewhere)]).assert_refused();
    let planted = b.at("data/browser-runs/r1/staging/x");
    sh(&l, &m1, r#"cp /bin/echo "$1""#, &[s(&planted)]).assert_refused();
    if Path::new(CHROME).exists() {
        sh(&l, &m1, r#"cat "$1" > /dev/null"#, &[CHROME]).assert_refused();
    }
}

/// Without a bridge the same binaries, as runtime paths, run, read and map: the bridge's denies
/// are what refuse them above.
#[test]
fn without_a_bridge_the_bundles_are_plain_runtime_paths() {
    let b = Bridged::new();
    let l = b.launcher(false);
    let helper = b.at("app/Clauseo.app/Contents/Helpers/clauseo-agent-browser");
    assert!(b.start(&l, &helper).assert_ok().stdout.contains(FLAGS));
    sh(&l, &b.at("m1"), r#"cat "$1" > /dev/null"#, &[s(&helper)]).assert_ok();
    let lib = b.at("app/Clauseo.app/Contents/Helpers/libhelper.dylib");
    assert_eq!(b.map(&l, &lib), "map=ok");
}

/// A headless shell the agent copies into the matter. The matter allows run (the base allows
/// running any program) but not map, and the agent profile allows no Chromium Mach name, so
/// the shell's `bootstrap_check_in` of its port broker fails and it dies at start, with no DOM.
/// The residual: a Chromium modified to run without its rendezvous server would get further;
/// `native_code_deny` (#810) is what would refuse running it at all.
#[test]
fn t9_a_headless_shell_in_the_matter_dies_at_start() {
    let Some(shell) = headless_shell() else {
        eprintln!("skipped: no chrome-headless-shell (set CLAUSEO_TEST_HEADLESS_SHELL)");
        return;
    };
    let w = World::new();
    let folder = shell.parent().unwrap();
    copy_tree(&w, folder, "m1/chs");
    let page = w.file("m1/page.html", "<html><body>t9-dom</body></html>\n");
    let (l, _) = launcher(w.builder().build().unwrap());
    let program = w.at("m1/chs").join(shell.file_name().unwrap());
    let url = format!("file://{}", s(&page));
    let flags = [
        "--headless",
        "--no-sandbox",
        "--disable-gpu",
        "--dump-dom",
        &url,
    ];
    let out = run(&l, s(&program), &flags, &w.at("m1"), &[]);
    assert_ne!(out.code, Some(0), "{out:#?}");
    assert!(
        !out.stdout.contains("t9-dom"),
        "the shell rendered the page: {out:#?}"
    );
}

/// A headless shell on this Mac: `CLAUSEO_TEST_HEADLESS_SHELL`, else one agent-browser or
/// Playwright downloaded.
pub(crate) fn headless_shell() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLAUSEO_TEST_HEADLESS_SHELL") {
        return Some(PathBuf::from(path));
    }
    let home = std::env::home_dir()?;
    for root in [".agent-browser/browsers", "Library/Caches/ms-playwright"] {
        let Ok(entries) = std::fs::read_dir(home.join(root)) else {
            continue;
        };
        for entry in entries.flatten() {
            let shell = entry
                .path()
                .join("chrome-headless-shell-mac-arm64/chrome-headless-shell");
            if shell.exists() {
                return Some(shell);
            }
        }
    }
    None
}

/// Copies the folder `from` to `to` (relative to the world), files keeping their modes.
fn copy_tree(w: &World, from: &Path, to: &str) {
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let rel = format!("{to}/{}", entry.file_name().to_str().unwrap());
        if entry.file_type().unwrap().is_dir() {
            w.dir(&rel);
            copy_tree(w, &entry.path(), &rel);
        } else {
            w.copy_in(&entry.path(), &rel);
        }
    }
}
