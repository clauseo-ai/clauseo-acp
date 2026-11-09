//! The browser profile (`macos::browser`, decision 0038) on this Mac: system tools and the
//! probe run through `BrowserLauncher` against the jail tests' world plus a run folder,
//! `data/browser-runs/r1/`. Matters, the home and other state folders are its deny roots.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use clauseo_process::{LaunchSpec, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_sandbox::macos::browser::{BrowserLauncher, BrowserPaths, RUN_SUBFOLDERS};

use crate::probe;
use crate::support::{Output, World, launcher, run};

/// Programs the tests run under the profile; each is an exec path of the profile.
const BASH: &str = "/bin/bash";
const CAT: &str = "/bin/cat";
const NC: &str = "/usr/bin/nc";
const SLEEP: &str = "/bin/sleep";

/// The world, its run folder, the profile's launcher and the loopback listeners it may reach
/// (the proxy's and the host bridge's) and may not (`other`).
pub(crate) struct Browser {
    pub(crate) w: World,
    pub(crate) run: PathBuf,
    pub(crate) launcher: BrowserLauncher,
    probe: PathBuf,
    proxy: TcpListener,
    pub(crate) bridge: TcpListener,
    other: TcpListener,
}

pub(crate) fn port(listener: &TcpListener) -> u16 {
    listener.local_addr().unwrap().port()
}

impl Browser {
    /// The world with a run folder, the probe copied to `bin/probe`, and a launcher whose
    /// Chrome is `bundle` (a stand-in headless shell's folder when `None`).
    pub(crate) fn new(bundle: Option<&Path>) -> Browser {
        Browser::with(bundle, &[])
    }

    /// [`Browser::new`] with `more` binaries the profile may run.
    pub(crate) fn with(bundle: Option<&Path>, more: &[PathBuf]) -> Browser {
        let w = World::new();
        for sub in RUN_SUBFOLDERS {
            w.dir(&format!("data/browser-runs/r1/{sub}"));
        }
        w.dir("chrome/chs");
        w.file("home/.ssh/id_ed25519", "private key\n");
        w.file("m1/brief.txt", "this matter\n");
        let probe = probe::copy_to(&w.dir("bin").join("probe"));
        let (proxy, bridge, other) = (listen(), listen(), listen());
        let paths = BrowserPaths {
            run_dir: w.at("data/browser-runs/r1"),
            chrome_bundle: bundle.map_or_else(|| w.at("chrome/chs"), Path::to_owned),
            exec_bins: [BASH, CAT, NC, SLEEP]
                .map(PathBuf::from)
                .into_iter()
                .chain([probe.clone()])
                .chain(more.iter().cloned())
                .collect(),
            proxy_port: port(&proxy),
            bridge_port: port(&bridge),
            deny_roots: [
                "m1",
                "m2",
                "home",
                "outside",
                "data/agent-state",
                "claude",
                "login",
            ]
            .map(|rel| w.at(rel))
            .to_vec(),
        };
        let launcher = BrowserLauncher::new(&paths, SpawnOptions::new()).unwrap();
        let run = launcher.run_dir().to_owned();
        Browser {
            w,
            run,
            launcher,
            probe,
            proxy,
            bridge,
            other,
        }
    }

    pub(crate) fn bash(&self, script: &str, args: &[&str]) -> Output {
        let mut all = vec!["-c", script, "bash"];
        all.extend_from_slice(args);
        run(&self.launcher, BASH, &all, &self.run, &[])
    }

    fn probe(&self, mode: &str, args: &[&str]) -> String {
        let out = run(
            &self.launcher,
            self.probe.to_str().unwrap(),
            &probe::ARGS,
            &self.run,
            &probe::env(mode, args),
        );
        probe::answer(&out.stdout)
    }
}

pub(crate) fn listen() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").unwrap()
}

pub(crate) fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn matters_the_home_and_other_state_are_neither_read_nor_written() {
    let b = Browser::new(None);
    let w = &b.w;
    for rel in [
        "m1/brief.txt",
        "m2/brief.txt",
        "home/secret.txt",
        "home/.ssh/id_ed25519",
        "data/agent-state/m2/notes/n.txt",
        "data/versions/v1.txt",
        "outside/secret.txt",
    ] {
        let out = b.bash(r#"cat "$1""#, &[s(&w.at(rel))]);
        out.assert_refused();
        assert!(
            !out.stdout.contains("secret") && !out.stdout.contains("matter"),
            "{out:#?}"
        );
    }
    for rel in [
        "m1/new.txt",
        "m2/new.txt",
        "home/new.txt",
        "home/.ssh/authorized_keys",
        "outside/new.txt",
        "data/new.txt",
        "data/browser-runs/new.txt",
        "new.txt",
    ] {
        b.bash(r#"echo x > "$1""#, &[s(&w.at(rel))])
            .assert_refused();
        assert!(!w.at(rel).exists(), "{rel} was written");
    }
    b.bash("echo x > /private/tmp/clauseo-browser-escape", &[])
        .assert_refused();
    // The run folder itself is the stack's.
    for sub in RUN_SUBFOLDERS {
        let file = b.run.join(sub).join("ok.txt");
        b.bash(r#"echo ok > "$1" && cat "$1""#, &[s(&file)])
            .assert_ok();
    }
}

#[test]
fn the_keychain_is_refused() {
    let b = Browser::new(None);
    b.bash("cat /Library/Keychains/System.keychain > /dev/null", &[])
        .assert_refused();
    let home = std::env::home_dir().unwrap();
    let answer = b.probe("marker", &[s(&b.run), s(&home.join("Library/Keychains"))]);
    assert!(answer.ends_with("read=false"), "{answer}");
}

#[test]
fn tcp_reaches_only_the_proxy_and_bridge_ports() {
    let b = Browser::new(None);
    let nc = |port: u16| {
        let port = port.to_string();
        run(
            &b.launcher,
            NC,
            &["-v", "-z", "-G", "5", "127.0.0.1", &port],
            &b.run,
            &[],
        )
    };
    nc(port(&b.proxy)).assert_ok();
    nc(port(&b.bridge)).assert_ok();
    nc(port(&b.other)).assert_refused();
}

#[test]
fn code_dropped_in_the_run_folder_neither_runs_nor_maps() {
    let b = Browser::new(None);
    for sub in ["staging", "tmp", "ab"] {
        let script =
            b.w.script(&format!("data/browser-runs/r1/{sub}/x"), "echo planted-ran");
        let binary = probe::copy_to(&b.run.join(sub).join("bin"));
        for planted in [&script, &binary] {
            let out = b.bash(r#""$1""#, &[s(planted)]);
            out.assert_refused();
            assert!(!out.stdout.contains("planted-ran"), "{out:#?}");
        }
        let lib = probe::dylib_to(&b.run.join(sub).join("lib.dylib"));
        let answer = b.probe("map", &[s(&lib)]);
        assert!(answer.contains("sandbox blocked mmap"), "{sub}: {answer}");
    }
}

/// Unix sockets are bound only at `sock/guard.sock` and in `ab/` (agent-browser's daemon,
/// Chrome's singleton): no other name in `sock/`, nothing elsewhere in the run.
#[test]
fn sockets_are_bound_only_at_the_guard_socket_and_in_ab() {
    let b = Browser::new(None);
    for (rel, want) in [
        ("sock/guard.sock", "bind=ok"),
        ("ab/daemon.sock", "bind=ok"),
        (
            "sock/other.sock",
            "bind=Operation not permitted (os error 1)",
        ),
        ("tmp/x.sock", "bind=Operation not permitted (os error 1)"),
        (
            "staging/x.sock",
            "bind=Operation not permitted (os error 1)",
        ),
    ] {
        let answer = b.probe("bind", &[s(&b.run.join(rel))]);
        assert_eq!(answer, want, "{rel}");
    }
}

/// The guard's marker (0038, D3): only the browser profile lets a write under
/// `<data>/browser-runs/` through, and it refuses a read of the home. `sandbox_check` answers
/// only for a path that exists, so each path checked does.
#[test]
fn the_marker_tells_the_browser_profile_from_the_agent_jail() {
    let b = Browser::new(None);
    let file = b.w.file("data/browser-runs/r1/staging/f", "");
    let home = std::env::home_dir().unwrap();
    let answer = b.probe("marker", &[s(&file), s(&home)]);
    assert_eq!(answer, "sandboxed=true write=true read=false");
    let ssh = b.w.at("home/.ssh");
    let answer = b.probe("marker", &[s(&b.run.join("tmp")), s(&ssh)]);
    assert_eq!(answer, "sandboxed=true write=true read=false");
    // The agent's jail: the probe in the matter, the data dir hidden.
    let agent_probe = probe::copy_to(&b.w.at("m1/probe"));
    let (agent, _) = launcher(b.w.builder().build().unwrap());
    let out = run(
        &agent,
        s(&agent_probe),
        &probe::ARGS,
        &b.w.at("m1"),
        &probe::env("marker", &[s(&file), s(&b.w.at("m1/brief.txt"))]),
    );
    assert_eq!(
        probe::answer(&out.stdout),
        "sandboxed=true write=false read=true"
    );
}

#[test]
fn the_browser_launcher_refuses_to_start_inside_a_sandbox() {
    let b = Browser::new(None);
    assert_eq!(b.probe("nest", &[]), "nest=Some(AlreadySandboxed)");
}

/// Only Chrome's port broker name may be looked up. The window server, `LaunchServices`'
/// database and network configuration, which an app-bundle Chrome needs, and any other name,
/// are refused (1100); the broker's name passes and is unknown here (1102). Pins the exact name:
/// a wider regex once matched every service.
#[test]
fn only_the_port_broker_name_is_looked_up() {
    let b = Browser::new(None);
    for name in [
        "com.apple.windowserver.active",
        "com.apple.lsd.mapdb",
        "com.apple.SystemConfiguration.configd",
    ] {
        assert_eq!(probe::lookup(name), 0, "the test itself reaches {name}");
        assert_eq!(b.probe("lookup", &[name]), "lookup=1100", "{name}");
    }
    let unknown = "com.clauseo.test.no-such-service";
    assert_eq!(b.probe("lookup", &[unknown]), "lookup=1100");
    let broker = "org.chromium.Chromium.MachPortRendezvousServer.1";
    assert_eq!(b.probe("lookup", &[broker]), "lookup=1102");
}

/// A process outside the sandbox: its arguments (`KERN_PROCARGS2`) and info (`proc_pidinfo`)
/// are refused, and `sandbox_check` says it is not sandboxed. The pid list works (numbers only,
/// as in #811), and the sandbox's own processes (here a `sleep` and its parent shell; in a run,
/// Chrome's renderers) read: their arguments and their parent, so the guard finds Chrome's
/// children by listing pids and reading each one's parent, and `sandbox_check` answers 1.
#[test]
fn outside_processes_are_hidden_and_the_sandbox_s_own_are_not() {
    let b = Browser::new(None);
    let spec = LaunchSpec::new(
        SLEEP.into(),
        vec!["60".into()],
        BTreeMap::new(),
        b.run.clone(),
    );
    let mut sleeper = PassthroughLauncher::new(SpawnOptions::new())
        .launch(&spec.unwrap())
        .unwrap();
    let pid = sleeper.handle.pid().to_string();
    let refused = "Operation not permitted (os error 1)";
    assert_eq!(probe::inspect("procargs", Path::new(&pid)), "ok");
    assert_ne!(probe::inspect("ppid", Path::new(&pid)), refused);
    assert_eq!(b.probe("procargs", &[&pid]), format!("procargs={refused}"));
    assert_eq!(b.probe("ppid", &[&pid]), format!("ppid={refused}"));
    assert_eq!(b.probe("listpids", &[]), "listpids=ok");
    assert_eq!(b.probe("pid-sandboxed", &[&pid]), "pid-sandboxed=false");
    let own = r#"/bin/sleep 5 & c=$!
        echo "SHELL $$"
        for mode in procargs ppid pid-sandboxed; do
            CLAUSEO_JAIL_PROBE=$mode CLAUSEO_JAIL_PROBE_1=$c "$1" probe::probe --exact --nocapture
        done
        kill $c"#;
    let out = b.bash(own, &[s(&b.probe)]);
    let shell = out
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("SHELL "))
        .unwrap();
    assert!(out.stdout.contains("PROBE procargs=ok"), "{out:#?}");
    assert!(
        out.stdout.contains(&format!("PROBE ppid={shell}")),
        "{out:#?}"
    );
    assert!(out.stdout.contains("PROBE pid-sandboxed=true"), "{out:#?}");
    clauseo_process::kill(sleeper.handle.pid());
    async_io::block_on(sleeper.handle.exited());
}

/// The guard `lstat`s every folder from `/` down to `sock/` before it binds, and the stack
/// `lstat`s `tmp/`: the run folder's ancestors give their metadata (literal rules), not their
/// entries.
#[test]
fn the_run_folder_s_ancestors_stat_but_do_not_list() {
    let b = Browser::new(None);
    for sub in ["sock", "tmp"] {
        assert_eq!(b.probe("lstat-up", &[s(&b.run.join(sub))]), "lstat-up=ok");
    }
    let refused = "readdir=Operation not permitted (os error 1)";
    let runs = b.run.parent().unwrap();
    for above in [runs, runs.parent().unwrap(), Path::new("/private/tmp")] {
        assert_eq!(
            b.probe("readdir", &[s(above)]),
            refused,
            "{}",
            above.display()
        );
    }
}

/// No inet bind at all, TCP or UDP, IPv4 or IPv6, on loopback or every address, on port 0
/// (agent-browser's fallback for its stream server) or a fixed one (its first try, and the
/// host bridge's port): each fails with EPERM. Seatbelt checks a bind by its port alone (its
/// `localhost` matches any address of the Mac), so the profile allows none (0038).
#[test]
fn every_inet_bind_is_refused() {
    let b = Browser::new(None);
    let free = port(&listen());
    let bridge = port(&b.bridge);
    for kind in ["tcp-bind", "udp-bind"] {
        let refused = format!("{kind}=Operation not permitted (os error 1)");
        for addr in [
            "127.0.0.1:0".to_owned(),
            "0.0.0.0:0".to_owned(),
            "[::]:0".to_owned(),
            "[::1]:0".to_owned(),
            format!("127.0.0.1:{free}"),
            format!("0.0.0.0:{free}"),
            format!("[::]:{free}"),
            format!("0.0.0.0:{bridge}"),
        ] {
            assert_eq!(b.probe(kind, &[&addr]), refused, "{addr}");
        }
    }
}

/// agent-browser's other listener, a Unix socket in `ab/`, still binds, takes a connection
/// and carries a byte both ways inside the profile.
#[test]
fn a_unix_listener_in_ab_binds_accepts_and_connects() {
    let b = Browser::new(None);
    let sock = b.run.join("ab/daemon.sock");
    assert_eq!(b.probe("unix-loop", &[s(&sock)]), "unix-loop=ok");
}
