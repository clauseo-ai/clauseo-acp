use std::ffi::OsStr;
use std::path::Path;

use super::{Os, Source, build, is_loopback_proxy, is_session};

/// Absolute by this platform's rules, so the same tests run on every OS.
fn abs(name: &str) -> String {
    if cfg!(windows) {
        format!(r"C:\x\{name}")
    } else {
        format!("/x/{name}")
    }
}

fn required() -> Vec<(&'static str, String)> {
    vec![
        ("AGENT_BROWSER_CONFIG", abs("agent-browser.json")),
        ("AGENT_BROWSER_PROXY", "http://127.0.0.1:8080".to_owned()),
        ("AGENT_BROWSER_EXECUTABLE_PATH", abs("chrome")),
        ("AGENT_BROWSER_SESSION", "s-1_A".to_owned()),
    ]
}

fn source_with(extra: &[(&str, &str)], drop: &str) -> Source {
    let required = required();
    let mut pairs: Vec<(&str, &str)> = required
        .iter()
        .filter(|(name, _)| *name != drop)
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    pairs.extend_from_slice(extra);
    Source::from_pairs(&pairs)
}

fn get<'a>(env: &'a std::collections::BTreeMap<String, std::ffi::OsString>, name: &str) -> &'a str {
    env.get(name).and_then(|v| v.to_str()).unwrap()
}

#[test]
fn proxy_must_be_loopback_http_with_a_plain_port() {
    for good in [
        "http://127.0.0.1:1",
        "http://127.0.0.1:8080",
        "http://127.0.0.1:65535",
    ] {
        assert!(is_loopback_proxy(OsStr::new(good)), "{good}");
    }
    for bad in [
        "",
        "http://127.0.0.1:",
        "http://127.0.0.1:0",
        "http://127.0.0.1:080",
        "http://127.0.0.1:65536",
        "http://127.0.0.1:100000",
        "http://127.0.0.1:+80",
        "http://127.0.0.1:80/",
        "http://127.0.0.1:80 ",
        " http://127.0.0.1:80",
        "http://localhost:80",
        "https://127.0.0.1:80",
        "HTTP://127.0.0.1:80",
        "http://user@127.0.0.1:80",
        "http://127.0.0.2:80",
        "http://[::1]:80",
        "socks5://127.0.0.1:80",
    ] {
        assert!(!is_loopback_proxy(OsStr::new(bad)), "{bad:?}");
    }
}

#[test]
fn session_is_short_and_plain() {
    assert!(is_session(OsStr::new("a")));
    assert!(is_session(OsStr::new(&"x".repeat(64))));
    assert!(is_session(OsStr::new("Ab-9_z")));
    for bad in ["", "a b", "a/b", "a.b", "a:b", "é", "a\n"] {
        assert!(!is_session(OsStr::new(bad)), "{bad:?}");
    }
    assert!(!is_session(OsStr::new(&"x".repeat(65))));
}

#[test]
fn each_required_variable_missing_is_named() {
    for (name, _) in required() {
        let error = build(&source_with(&[], name), Os::CURRENT, Path::new(&abs("cwd")))
            .unwrap_err()
            .to_string();
        assert_eq!(error, format!("{name} is not set; the host sets it"));
    }
}

#[test]
fn each_required_variable_invalid_is_named() {
    let cwd = abs("cwd");
    for (name, bad, words) in [
        (
            "AGENT_BROWSER_CONFIG",
            "relative/agent-browser.json",
            "must be an absolute path",
        ),
        ("AGENT_BROWSER_CONFIG", "", "must be an absolute path"),
        (
            "AGENT_BROWSER_EXECUTABLE_PATH",
            "chrome",
            "must be an absolute path",
        ),
        (
            "AGENT_BROWSER_PROXY",
            "http://evil:1",
            "must be exactly http://127.0.0.1:<port>",
        ),
        ("AGENT_BROWSER_SESSION", "../x", "must be 1 to 64 of"),
    ] {
        let source = source_with(&[(name, bad)], name);
        let error = build(&source, Os::CURRENT, Path::new(&cwd))
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(&format!("{name} {words}")), "{error}");
    }
}

#[test]
fn unix_map_is_exactly_the_allow_list() {
    let source = source_with(
        &[
            ("LANG", "en_IN.UTF-8"),
            ("lang", "xx"),
            ("HTTP_PROXY", "http://evil:1"),
            ("NO_PROXY", "*"),
            ("AGENT_BROWSER_ARGS", "--foo"),
            ("AGENT_BROWSER_NAMESPACE", "other"),
            ("AGENT_BROWSER_IDLE_TIMEOUT_MS", "1"),
            ("PATH", "/evil"),
            ("HOME", "/Users/lawyer"),
        ],
        "",
    );
    let cwd = Path::new("/state/browser");
    let env = build(&source, Os::Unix, cwd).unwrap();
    let mut names: Vec<&str> = env.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "AGENT_BROWSER_ARGS",
            "AGENT_BROWSER_CONFIG",
            "AGENT_BROWSER_DEFAULT_TIMEOUT",
            "AGENT_BROWSER_DOWNLOAD_PATH",
            "AGENT_BROWSER_EXECUTABLE_PATH",
            "AGENT_BROWSER_IDLE_TIMEOUT_MS",
            "AGENT_BROWSER_MAX_OUTPUT",
            "AGENT_BROWSER_NAMESPACE",
            "AGENT_BROWSER_PROXY",
            "AGENT_BROWSER_PROXY_BYPASS",
            "AGENT_BROWSER_SCREENSHOT_DIR",
            "AGENT_BROWSER_SESSION",
            "AGENT_BROWSER_SOCKET_DIR",
            "ALL_PROXY",
            "HOME",
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "LANG",
            "PATH",
            "TMPDIR",
            "all_proxy",
            "http_proxy",
            "https_proxy",
        ]
    );
    assert_eq!(get(&env, "AGENT_BROWSER_NAMESPACE"), "s-1_A");
    assert_eq!(get(&env, "HTTP_PROXY"), "http://127.0.0.1:8080");
    assert_eq!(get(&env, "http_proxy"), "http://127.0.0.1:8080");
    assert_eq!(get(&env, "AGENT_BROWSER_IDLE_TIMEOUT_MS"), "600000");
    assert_eq!(
        get(&env, "AGENT_BROWSER_ARGS"),
        "--force-webrtc-ip-handling-policy=disable_non_proxied_udp"
    );
    assert_eq!(get(&env, "LANG"), "en_IN.UTF-8");
    assert_eq!(get(&env, "PATH"), "/usr/bin:/bin:/usr/sbin:/sbin");
    assert_eq!(
        &env["HOME"],
        cwd.join("home").as_os_str(),
        "HOME is the state folder's, never the lawyer's"
    );
}

#[test]
fn windows_names_match_without_case_and_appear_once() {
    let source = source_with(
        &[
            ("SYSTEMROOT", r"D:\Win"),
            ("SystemRoot", r"E:\Other"),
            ("comspec", r"D:\Win\system32\cmd.exe"),
            ("Path", r"C:\evil"),
            ("no_proxy", "*"),
            ("LANG", "en"),
            ("ProgramFiles(x86)", r"D:\PF86"),
        ],
        "",
    );
    let cwd = Path::new("/state/browser");
    let env = build(&source, Os::Windows, cwd).unwrap();
    // The first match in the OS's order wins, under the allow-list's spelling.
    assert_eq!(get(&env, "SystemRoot"), r"D:\Win");
    assert_eq!(get(&env, "ComSpec"), r"D:\Win\system32\cmd.exe");
    assert_eq!(get(&env, "ProgramFiles(x86)"), r"D:\PF86");
    assert_eq!(get(&env, "PATH"), r"D:\Win\System32;D:\Win");
    for absent in [
        "SYSTEMROOT",
        "comspec",
        "Path",
        "no_proxy",
        "LANG",
        "HOME",
        "TMPDIR",
    ] {
        assert!(!env.contains_key(absent), "{absent}");
    }
    for lower in ["http_proxy", "https_proxy", "all_proxy"] {
        assert!(
            !env.contains_key(lower),
            "{lower} would be a second spelling on Windows"
        );
    }
    assert_eq!(&env["TEMP"], cwd.join("tmp").as_os_str());
    assert_eq!(&env["TMP"], cwd.join("tmp").as_os_str());
    // No two names in the map are the same name on Windows.
    let mut folded: Vec<String> = env.keys().map(|k| k.to_ascii_uppercase()).collect();
    let count = folded.len();
    folded.sort();
    folded.dedup();
    assert_eq!(folded.len(), count);
}

#[test]
fn windows_path_falls_back_to_c_windows() {
    let env = build(&source_with(&[], ""), Os::Windows, Path::new("/s")).unwrap();
    assert_eq!(get(&env, "PATH"), r"C:\Windows\System32;C:\Windows");
    assert!(!env.contains_key("SystemRoot"));
}
