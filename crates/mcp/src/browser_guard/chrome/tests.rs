//! The endpoint check and Chrome's arguments.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::{Spec, check_endpoint, chrome_args};

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
