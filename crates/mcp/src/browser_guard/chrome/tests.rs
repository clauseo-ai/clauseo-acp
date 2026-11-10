//! The endpoint check and Chrome's arguments.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::{Spec, check_endpoint, chrome_args, ready_profile, sandbox};

#[test]
fn the_endpoint_must_be_loopback_on_the_claimed_port() {
    let good = "ws://127.0.0.1:61101/devtools/browser/0b1e-77AF";
    assert_eq!(check_endpoint(good, 61_101), Ok(()));
    assert_eq!(
        check_endpoint("ws://127.0.0.1:47120/devtools/browser/abc", 61_101),
        Err("Chrome listens on debugging port 47120, not the claimed port 61101".to_owned())
    );
    for bad in [
        "ws://localhost:61101/devtools/browser/abc",
        "ws://127.0.0.1:61101/devtools/page/abc",
        "ws://127.0.0.1:61101/devtools/browser/",
        "ws://127.0.0.1:61101/devtools/browser/a/b",
        "ws://127.0.0.1:61101/devtools/browser/a?b",
        "wss://127.0.0.1:61101/devtools/browser/abc",
        "ws://127.0.0.1:61101",
    ] {
        let refusal = check_endpoint(bad, 61_101).unwrap_err();
        assert!(!refusal.contains("ws://"), "names no endpoint: {refusal}");
    }
}

#[test]
fn chrome_gets_the_proxy_the_profile_and_the_claimed_port_never_port_zero() {
    let env = BTreeMap::new();
    let spec = Spec {
        executable: Path::new("/chrome"),
        cwd: Path::new("/state/browser"),
        profile: Path::new("/state/browser/chrome-profile-1-0000abcd"),
        cdp_port: 61_101,
        proxy: OsStr::new("http://127.0.0.1:8080"),
        env: &env,
    };
    let args = chrome_args(
        &spec,
        Path::new("/state/browser/chrome-profile-1-0000abcd"),
        sandbox::Host::now(),
    );
    let has = |arg: &str| args.iter().any(|a| a == &OsString::from(arg));
    for arg in [
        "--headless=new",
        "--proxy-server=http://127.0.0.1:8080",
        "--proxy-bypass-list=<-loopback>",
        "--user-data-dir=/state/browser/chrome-profile-1-0000abcd",
        "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
        "--remote-debugging-port=61101",
        "--disable-background-networking",
    ] {
        assert!(has(arg), "{arg} in {args:?}");
    }
    let debugging = args
        .iter()
        .filter(|a| a.to_string_lossy().starts_with("--remote-debugging"))
        .count();
    assert_eq!(debugging, 1, "{args:?}");
}

#[test]
fn chrome_gets_one_feature_list_of_each_kind_and_no_weakened_security() {
    let env = BTreeMap::new();
    let spec = Spec {
        executable: Path::new("/chrome"),
        cwd: Path::new("/state/browser"),
        profile: Path::new("/state/browser/chrome-profile-1-0000abcd"),
        cdp_port: 61_101,
        proxy: OsStr::new("http://127.0.0.1:8080"),
        env: &env,
    };
    let args: Vec<String> = chrome_args(&spec, spec.profile, sandbox::Host::now())
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    // Chrome reads only the last of each, so a second one would drop the first's features.
    for (switch, wanted) in [
        (
            "--disable-features=",
            ["Translate", "NetworkTimeServiceQuerying", "AimEnabled"],
        ),
        (
            "--enable-features=",
            ["NetworkService", "WebMCPTesting", "DevToolsWebMCPSupport"],
        ),
    ] {
        let lists: Vec<&String> = args.iter().filter(|a| a.starts_with(switch)).collect();
        assert_eq!(lists.len(), 1, "{args:?}");
        let features: Vec<&str> = lists[0][switch.len()..].split(',').collect();
        for feature in wanted {
            assert!(features.contains(&feature), "{feature} in {features:?}");
        }
        for isolation in ["IsolateOrigins", "SitePerProcess", "site-per-process"] {
            assert!(
                !features.contains(&isolation),
                "{isolation} in {features:?}"
            );
        }
    }
    for weakening in [
        "--disable-web-security",
        "--ignore-certificate-errors",
        "--disable-site-isolation-trials",
        "--allow-running-insecure-content",
    ] {
        assert!(
            !args.iter().any(|a| a.starts_with(weakening)),
            "{weakening} in {args:?}"
        );
    }
    assert!(args.contains(&"--isolate-origins=https://accounts.google.com".to_owned()));
    let updater: Vec<&String> = args
        .iter()
        .filter(|a| a.starts_with("--component-updater="))
        .collect();
    assert_eq!(
        updater,
        ["--component-updater=url-source=https://update.invalid:1/"],
        "one updater switch, or Chrome reads only the last"
    );
    let preferences = super::quiet::preferences();
    assert_eq!(preferences["safebrowsing"]["enabled"], false);
    assert_eq!(preferences["net"]["network_prediction_options"], 2);
}

/// On every OS, as root and in a container, Chrome's arguments name no switch or feature that
/// weakens its sandbox, by the exact check and by the coarse one.
#[test]
fn chrome_never_gets_a_switch_that_weakens_its_sandbox_on_any_host() {
    let env = BTreeMap::new();
    let spec = Spec {
        executable: Path::new("/chrome"),
        cwd: Path::new("/state/browser"),
        profile: Path::new("/state/browser/chrome-profile-1-0000abcd"),
        cdp_port: 61_101,
        proxy: OsStr::new("http://127.0.0.1:8080"),
        env: &env,
    };
    for linux in [false, true] {
        for root in [false, true] {
            for container in [false, true] {
                let host = sandbox::Host {
                    linux,
                    root,
                    container,
                };
                let args = chrome_args(&spec, spec.profile, host);
                for arg in &args {
                    assert_eq!(sandbox::weakening(arg), None, "{arg:?} for {host:?}");
                    assert_eq!(sandbox::weakening_in(arg), None, "{arg:?} for {host:?}");
                }
                for switch in sandbox::SWITCHES {
                    let lower = |a: &OsString| a.to_string_lossy().to_ascii_lowercase();
                    assert!(
                        !args.iter().any(|a| lower(a).contains(switch)),
                        "{switch} for {host:?}"
                    );
                }
            }
        }
    }
}

/// The profile's own preferences, `quiet`'s and `logon`'s share no top-level key, so none may
/// hide another's: all arrive whole.
#[test]
fn the_profile_keeps_the_download_preferences_and_quiets_whole() {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-prefs-{}", std::process::id()));
    let (profile, staging, downloads) = (dir.join("p"), dir.join("p/s"), dir.join("d"));
    ready_profile(&profile, &staging, &downloads).unwrap();
    let written = std::fs::read_to_string(profile.join("Default").join("Preferences")).unwrap();
    crate::write::remove_dir_all(&dir).unwrap();
    let written: serde_json::Value = serde_json::from_str(&written).unwrap();
    let quiet = super::quiet::preferences();
    let quiet = quiet.as_object().unwrap();
    let logon = super::super::logon::preferences();
    let logon = logon.as_object().unwrap();
    for own in ["download", "savefile", "profile"] {
        assert!(!quiet.contains_key(own), "quiet sets {own}");
        assert!(!logon.contains_key(own), "logon sets {own}");
    }
    assert!(
        logon.keys().all(|key| !quiet.contains_key(key)),
        "{logon:?}"
    );
    assert_eq!(
        written.as_object().unwrap().len(),
        3 + quiet.len() + logon.len(),
        "{written}"
    );
    assert_eq!(written["download"]["prompt_for_download"], false);
    assert_eq!(
        written["savefile"]["default_directory"],
        downloads.to_str().unwrap()
    );
    let exceptions = &written["profile"]["content_settings"]["exceptions"];
    assert_eq!(exceptions["mixed_script"]["http://*:*,*"]["setting"], 1);
    for (key, value) in quiet {
        assert_eq!(&written[key], value, "{key}");
    }
}

/// A restart keeps the profile Chrome rewrote, its own entries included, and puts `quiet`'s
/// preferences back over whatever Chrome wrote for them, so every start gets them.
#[test]
fn a_restart_puts_the_quiet_preferences_back_into_the_kept_profile() {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-prefs-again-{}", std::process::id()));
    let (profile, staging, downloads) = (dir.join("p"), dir.join("p/s"), dir.join("d"));
    ready_profile(&profile, &staging, &downloads).unwrap();
    let file = profile.join("Default").join("Preferences");
    // Chrome's rewrite: its own keys, and one of quiet's changed back.
    let rewritten = serde_json::json!({
        "download": {"default_directory": staging, "prompt_for_download": false},
        "safebrowsing": {"enabled": true, "other": 7},
        "chrome_own": {"kept": true},
    });
    crate::write::write(&file, rewritten.to_string()).unwrap();
    ready_profile(&profile, &staging, &downloads).unwrap();
    let written = std::fs::read_to_string(&file).unwrap();
    crate::write::remove_dir_all(&dir).unwrap();
    let written: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(written["chrome_own"]["kept"], true, "{written}");
    assert_eq!(written["safebrowsing"]["other"], 7, "{written}");
    assert_eq!(written["download"]["prompt_for_download"], false);
    let quiet = super::quiet::preferences();
    for (key, value) in quiet.as_object().unwrap() {
        for (inner, setting) in value.as_object().unwrap() {
            assert_eq!(&written[key][inner], setting, "{key}.{inner}");
        }
    }
}

/// On Linux `--gpu-sandbox-failures-fatal` is always among Chrome's arguments, bare (a value
/// such as `=no` would turn it off), so a GPU sandbox that cannot start ends the GPU process
/// instead of leaving it unsandboxed; elsewhere Chromium ignores it and it is not passed.
#[test]
fn chrome_on_linux_always_gets_gpu_sandbox_failures_fatal() {
    let env = BTreeMap::new();
    let spec = Spec {
        executable: Path::new("/chrome"),
        cwd: Path::new("/state/browser"),
        profile: Path::new("/state/browser/chrome-profile-1-0000abcd"),
        cdp_port: 61_101,
        proxy: OsStr::new("http://127.0.0.1:8080"),
        env: &env,
    };
    for linux in [false, true] {
        for root in [false, true] {
            for container in [false, true] {
                let host = sandbox::Host {
                    linux,
                    root,
                    container,
                };
                let args = chrome_args(&spec, spec.profile, host);
                let fatal = args
                    .iter()
                    .filter(|arg| arg.to_string_lossy().contains("gpu-sandbox-failures-fatal"))
                    .collect::<Vec<_>>();
                let expected: &[&OsStr] = if linux {
                    &[OsStr::new("--gpu-sandbox-failures-fatal")]
                } else {
                    &[]
                };
                assert_eq!(fatal, expected, "{host:?}");
            }
        }
    }
}

/// On Linux as root `launch` refuses with the sandbox's own error before anything happens: no
/// profile is made and Chrome (here a program that would make a file) never starts.
#[cfg(unix)]
#[test]
fn launch_as_root_on_linux_is_refused_before_anything_starts() {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-root-{}", std::process::id()));
    let profile = dir.join("p");
    let env = BTreeMap::new();
    let spec = Spec {
        executable: Path::new("/bin/touch"),
        cwd: &dir,
        profile: &profile,
        cdp_port: 61_101,
        proxy: OsStr::new("http://127.0.0.1:8080"),
        env: &env,
    };
    for container in [false, true] {
        let host = sandbox::Host {
            linux: true,
            root: true,
            container,
        };
        let refused = super::launch_on(&spec, host).err();
        assert_eq!(refused, Some(super::StartError::Sandbox(None)), "{host:?}");
        assert!(!dir.exists(), "{host:?}: something was made");
    }
}
