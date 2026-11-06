//! The endpoint check and Chrome's arguments.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::{Spec, check_endpoint, chrome_args, fresh_profile};

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
    let args = chrome_args(&spec, Path::new("/state/browser/chrome-profile-1-0000abcd"));
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
    let args: Vec<String> = chrome_args(&spec, spec.profile)
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

/// The profile's own preferences and `quiet`'s are merged at the top level only, so neither may
/// hide the other's keys: both arrive whole.
#[test]
fn the_profile_keeps_the_download_preferences_and_quiets_whole() {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-prefs-{}", std::process::id()));
    let (profile, staging, downloads) = (dir.join("p"), dir.join("p/s"), dir.join("d"));
    fresh_profile(&profile, &staging, &downloads).unwrap();
    let written = std::fs::read_to_string(profile.join("Default").join("Preferences")).unwrap();
    crate::write::remove_dir_all(&dir).unwrap();
    let written: serde_json::Value = serde_json::from_str(&written).unwrap();
    let quiet = super::quiet::preferences();
    let quiet = quiet.as_object().unwrap();
    for own in ["download", "savefile", "profile"] {
        assert!(!quiet.contains_key(own), "quiet sets {own}");
    }
    assert_eq!(
        written.as_object().unwrap().len(),
        3 + quiet.len(),
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
