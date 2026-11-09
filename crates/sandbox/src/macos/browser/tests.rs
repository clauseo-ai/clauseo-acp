use std::path::PathBuf;

use super::*;
use crate::test_support::TempDir;

/// A data dir with one run folder, a stand-in headless shell's folder and Chrome app, a guard
/// binary, a matter and a home.
struct World {
    dir: TempDir,
}

impl World {
    fn new(test: &str) -> World {
        let dir = TempDir::new(test);
        for sub in RUN_SUBFOLDERS {
            dir.dir(&format!("data/browser-runs/r1/{sub}"));
        }
        for name in [
            "chs",
            "Chrome.app/Contents/MacOS",
            "bin",
            "matter",
            "home/.ssh",
        ] {
            dir.dir(name);
        }
        dir.file("bin/guard", "");
        World { dir }
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn paths(&self) -> BrowserPaths {
        BrowserPaths {
            run_dir: self.at("data/browser-runs/r1"),
            chrome_bundle: self.at("chs"),
            exec_bins: vec![self.at("bin/guard")],
            proxy_port: 41_001,
            cdp_port: 41_002,
            stream_port: 41_003,
            deny_roots: vec![self.at("matter"), self.at("home/.ssh"), self.at("other")],
        }
    }
}

/// Each rule of the text, from its `(` to the one that closes it, comments left out.
fn rules(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0_i32;
    for line in text.lines().filter(|l| !l.trim_start().starts_with(';')) {
        for c in line.chars() {
            if c == '(' {
                depth += 1;
            }
            if depth > 0 {
                current.push(c);
            }
            if c == ')' {
                depth -= 1;
                if depth == 0 {
                    out.push(std::mem::take(&mut current));
                }
            }
        }
        if depth > 0 {
            current.push('\n');
        }
    }
    out
}

#[test]
fn the_text_is_allows_then_denies_and_every_deny_is_tagged() {
    let w = World::new("browser-order");
    let text = profile(&w.paths()).unwrap();
    let all = rules(&text);
    assert_eq!(all[0], "(version 1)");
    assert_eq!(all[1], format!("(deny default (with message \"{TAG}\"))"));
    assert!(
        all[2].starts_with("(deny file-map-executable"),
        "{}",
        all[2]
    );
    let first_deny = all
        .iter()
        .skip(3)
        .position(|r| r.starts_with("(deny"))
        .unwrap();
    for rule in all.iter().skip(3 + first_deny) {
        assert!(rule.starts_with("(deny"), "an allow after a deny: {rule}");
    }
    for rule in all.iter().filter(|r| r.starts_with("(deny")) {
        assert!(
            rule.contains(&format!("(with message \"{TAG}\")")),
            "{rule}"
        );
    }
}

#[test]
fn code_runs_only_from_the_bundle_the_binaries_and_the_system() {
    let w = World::new("browser-code");
    let text = profile(&w.paths()).unwrap();
    let bundle = format!("(subpath \"{}\")", w.at("chs").display());
    let guard = format!("(subpath \"{}\")", w.at("bin/guard").display());
    let canonical = dunce::canonicalize(w.at("bin/guard")).unwrap();
    let guard_canonical = format!("(subpath \"{}\")", canonical.display());
    for rule in rules(&text).iter().filter(|r| r.starts_with("(allow")) {
        if !(rule.contains("process-exec") || rule.contains("file-map-executable")) {
            continue;
        }
        for filter in rule.lines().skip(1) {
            let filter = filter.trim().trim_end_matches(')');
            let system = filter == "(subpath \"/System\"" || filter == "(subpath \"/usr/lib\"";
            let ours = [&bundle, &guard, &guard_canonical]
                .iter()
                .any(|f| f.trim_end_matches(')') == filter)
                || filter.ends_with("/chs\"");
            assert!(system || ours, "code allowed from {filter}");
        }
    }
    let run = dunce::canonicalize(w.at("data/browser-runs/r1")).unwrap();
    assert!(text.contains(&format!(
        "(deny process-exec file-map-executable\n  (subpath \"{}\")",
        run.display()
    )));
}

#[test]
fn the_network_is_three_loopback_ports_over_ipv4_the_guard_socket_and_ab() {
    let w = World::new("browser-net");
    let text = profile(&w.paths()).unwrap();
    for port in [41_001, 41_002, 41_003] {
        assert!(text.contains(&format!("(remote tcp4 \"localhost:{port}\")")));
    }
    assert!(
        !text.contains("(local tcp4 \"localhost:41001\")"),
        "binds the proxy port"
    );
    for port in [41_002, 41_003] {
        assert!(text.contains(&format!("(local tcp4 \"localhost:{port}\")")));
    }
    for wide in ["localhost:*", "*:", "(remote ip", "(local ip", "udp", "ip6"] {
        assert!(!text.contains(wide), "{wide}");
    }
    let run = dunce::canonicalize(w.at("data/browser-runs/r1")).unwrap();
    let mut sockets: Vec<&str> = text.lines().filter(|l| l.contains("unix-socket")).collect();
    sockets.sort_unstable();
    sockets.dedup();
    for line in &sockets {
        let sock = format!(
            "(path-literal \"{}\")",
            run.join("sock/guard.sock").display()
        );
        let ab = format!("(subpath \"{}\")", run.join("ab").display());
        assert!(line.contains(&sock) || line.contains(&ab), "{line}");
    }
    assert_eq!(sockets.len(), 4, "{sockets:?}");
}

#[test]
fn no_gpu_window_server_or_keychain_is_allowed() {
    let w = World::new("browser-refused");
    let text = profile(&w.paths()).unwrap();
    for rule in rules(&text).iter().filter(|r| r.starts_with("(allow")) {
        for name in [
            "IOSurface",
            "AGX",
            "IOAccelerator",
            "CARenderServer",
            "Keychains",
            "SecurityServer",
            "securityd",
            "nvram",
            "iokit-get-properties",
        ] {
            assert!(!rule.contains(name), "an allow names {name}: {rule}");
        }
    }
    for deny in [
        "(deny process-info*\n  (require-not (target same-sandbox))",
        "(deny nvram*",
        "(deny iokit-get-properties",
        "(iokit-user-client-class \"IOSurfaceRootUserClient\")",
        "(subpath \"/Library/Keychains\")",
        "(global-name \"com.apple.SecurityServer\")",
    ] {
        assert!(text.contains(deny), "{deny}");
    }
}

/// Runs `profile` on the world's paths changed by `change`.
fn refused(w: &World, change: impl FnOnce(&mut BrowserPaths)) -> BrowserProfileError {
    let mut paths = w.paths();
    change(&mut paths);
    profile(&paths).unwrap_err()
}

#[test]
fn bad_paths_are_refused() {
    let w = World::new("browser-refusals");
    w.dir.dir("matter/browser-runs/r2");
    w.dir.dir("data/runs/r3");
    w.dir.dir("data/browser-runs/q\"uote");
    w.dir.file("data/browser-runs/r1/staging/planted", "");
    let e = refused(&w, |p| p.run_dir = PathBuf::from("data/browser-runs/r1"));
    assert!(matches!(e, BrowserProfileError::Relative { .. }), "{e}");
    let e = refused(&w, |p| p.exec_bins.push(PathBuf::from("bin/guard")));
    assert!(matches!(e, BrowserProfileError::Relative { .. }), "{e}");
    let e = refused(&w, |p| p.deny_roots.push(PathBuf::from("matter")));
    assert!(matches!(e, BrowserProfileError::Relative { .. }), "{e}");
    let e = refused(&w, |p| p.run_dir = w.at("data/runs/r3"));
    assert!(matches!(e, BrowserProfileError::NotARunFolder(_)), "{e}");
    let e = refused(&w, |p| p.run_dir = w.at("data/browser-runs/missing"));
    assert!(matches!(e, BrowserProfileError::Unresolved { .. }), "{e}");
    // The run folder inside a matter, holding one, or being one.
    let e = refused(&w, |p| p.run_dir = w.at("matter/browser-runs/r2"));
    assert!(matches!(e, BrowserProfileError::InDenyRoot { .. }), "{e}");
    let e = refused(&w, |p| {
        p.deny_roots.push(w.at("data/browser-runs/r1/profile/m"));
    });
    assert!(matches!(e, BrowserProfileError::InDenyRoot { .. }), "{e}");
    let e = refused(&w, |p| p.deny_roots.push(w.at("data/browser-runs/r1")));
    assert!(matches!(e, BrowserProfileError::InDenyRoot { .. }), "{e}");
    // A binary in a matter, and one the stack itself could have written.
    let e = refused(&w, |p| p.exec_bins.push(w.at("matter")));
    assert!(matches!(e, BrowserProfileError::InDenyRoot { .. }), "{e}");
    let e = refused(&w, |p| {
        p.exec_bins
            .push(w.at("data/browser-runs/r1/staging/planted"));
    });
    assert!(matches!(e, BrowserProfileError::InRunFolder { .. }), "{e}");
    let e = refused(&w, |p| p.chrome_bundle = w.at("data/browser-runs/r1"));
    assert!(matches!(e, BrowserProfileError::InRunFolder { .. }), "{e}");
    let e = refused(&w, |p| p.run_dir = w.at("data/browser-runs/q\"uote"));
    assert!(matches!(e, BrowserProfileError::Unquotable { .. }), "{e}");
    let e = refused(&w, |p| p.stream_port = 0);
    assert!(matches!(e, BrowserProfileError::ZeroPort(_)), "{e}");
}

#[test]
fn a_deny_root_is_compared_in_any_letter_case_on_macos() {
    let w = World::new("browser-case");
    let mut paths = w.paths();
    paths.deny_roots.push(w.at("DATA/BROWSER-RUNS"));
    let result = profile(&paths);
    if cfg!(target_os = "macos") {
        let e = result.unwrap_err();
        assert!(matches!(e, BrowserProfileError::InDenyRoot { .. }), "{e}");
    } else {
        // Elsewhere names compare exactly, so the upper-case folder is another folder.
        result.unwrap();
    }
}

/// Only chrome-headless-shell is supported on macOS (the orchestrator's ruling): an app bundle,
/// a folder inside one, or a link to one is refused, whatever its letter case.
#[test]
fn an_app_bundle_is_refused() {
    let w = World::new("browser-app");
    w.dir.dir("Chrome.APP");
    #[cfg(unix)]
    w.dir.symlink("linked-chrome", &w.at("Chrome.app"));
    let link = if cfg!(unix) {
        "linked-chrome"
    } else {
        "Chrome.app"
    };
    for rel in [
        "Chrome.app",
        "Chrome.app/Contents/MacOS",
        "Chrome.APP",
        link,
    ] {
        let e = refused(&w, |p| p.chrome_bundle = w.at(rel));
        assert!(matches!(e, BrowserProfileError::AppBundle(_)), "{rel}: {e}");
        assert!(e.to_string().contains("only chrome-headless-shell"), "{e}");
    }
}

/// The headless shell needs no Mach service but its own port broker: no allow names the
/// services an app-bundle Chrome would want, and the window server is denied by name.
#[test]
fn only_the_port_broker_is_looked_up_and_the_window_server_is_denied() {
    let w = World::new("browser-mach");
    let text = profile(&w.paths()).unwrap();
    for rule in rules(&text).iter().filter(|r| r.starts_with("(allow")) {
        for name in [
            "com.apple.coreservices.launchservicesd",
            "com.apple.lsd.",
            "com.apple.CoreServices.coreservicesd",
            "com.apple.SystemConfiguration.configd",
            "com.apple.windowserver",
        ] {
            assert!(!rule.contains(name), "an allow names {name}: {rule}");
        }
    }
    let mach: Vec<String> = rules(&text)
        .into_iter()
        .filter(|r| r.starts_with("(allow mach-"))
        .collect();
    assert_eq!(
        mach,
        ["(allow mach-register mach-lookup\n  (global-name-regex \
          #\"^org\\.chromium\\.Chromium\\.MachPortRendezvousServer\\.[0-9]+$\"))"]
    );
    let window_server = rules(&text)
        .into_iter()
        .find(|r| r.contains("(global-name-prefix \"com.apple.windowserver\")"))
        .unwrap();
    assert!(
        window_server.starts_with("(deny mach-lookup"),
        "{window_server}"
    );
}
