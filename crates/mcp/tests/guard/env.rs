//! The child's environment: whatever the guard's own environment holds, the child sees exactly
//! the allow-list, Clauseo's values and the given working folder.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;

use serde_json::Value;

use crate::support::{CDP_PORT, Guard, PROXY, Scratch, example, guard_command, host_env};
use crate::write;

/// What the agent's environment may hold that must never reach agent-browser. The host's own
/// inputs (`AGENT_BROWSER_SOCKET_DIR` on Unix, `AGENT_BROWSER_STREAM_PORT`) are not here: the
/// host sets them on top, and `usage.rs` tests their refusals.
const HOSTILE: &[(&str, &str)] = &[
    ("HTTP_PROXY", "http://evil:1"),
    ("https_proxy", "http://evil:2"),
    ("NO_PROXY", "*"),
    ("no_proxy", "*"),
    ("AGENT_BROWSER_PROVIDER", "browserbase"),
    ("AGENT_BROWSER_PROFILE", "/Users/lawyer/Library/Chrome"),
    ("AGENT_BROWSER_CDP", "9222"),
    ("AGENT_BROWSER_ARGS", "--foo"),
    ("AGENT_BROWSER_DAEMON", "1"),
    ("AGENT_BROWSER_ENGINE", "lightpanda"),
    ("AGENT_BROWSER_NAMESPACE", "other"),
    ("AGENT_BROWSER_PROXY_BYPASS", "*"),
    ("AGENT_BROWSER_IDLE_TIMEOUT_MS", "1"),
    ("AGENT_BROWSER_SCREENSHOT_DIR", "/elsewhere"),
    ("AI_GATEWAY_API_KEY", "secret-for-the-test"),
    ("CI", "1"),
    ("XDG_RUNTIME_DIR", "/run/user/1000"),
    ("PATH", "/evil/bin"),
    ("HOME", "/Users/lawyer"),
    ("TMPDIR", "/tmp/elsewhere"),
];

/// The map the child must see, built from the brief's list for this OS.
fn expected(scratch: &Scratch) -> BTreeMap<String, String> {
    let cwd = scratch.cwd();
    let under = |name: &str| cwd.join(name).to_str().unwrap().to_owned();
    let mut map: BTreeMap<String, String> = host_env(scratch).into_iter().collect();
    let fixed = [
        ("AGENT_BROWSER_PROXY_BYPASS", "<-loopback>"),
        ("AGENT_BROWSER_IDLE_TIMEOUT_MS", "600000"),
        ("AGENT_BROWSER_MAX_OUTPUT", "25000"),
        ("AGENT_BROWSER_DEFAULT_TIMEOUT", "30000"),
        ("HTTP_PROXY", PROXY),
        ("HTTPS_PROXY", PROXY),
        ("ALL_PROXY", PROXY),
    ];
    for (name, value) in fixed {
        map.insert(name.to_owned(), value.to_owned());
    }
    // The WebRTC rule, then the host's CDP port, one per line.
    map.insert(
        "AGENT_BROWSER_ARGS".to_owned(),
        format!(
            "--force-webrtc-ip-handling-policy=disable_non_proxied_udp\n--remote-debugging-port={CDP_PORT}"
        ),
    );
    map.insert("AGENT_BROWSER_DOWNLOAD_PATH".to_owned(), under("downloads"));
    map.insert(
        "AGENT_BROWSER_SCREENSHOT_DIR".to_owned(),
        under("screenshots"),
    );
    if cfg!(windows) {
        map.insert("TEMP".to_owned(), under("tmp"));
        map.insert("TMP".to_owned(), under("tmp"));
        map.insert("AGENT_BROWSER_SOCKET_DIR".to_owned(), under("run"));
        for name in [
            "SystemRoot",
            "windir",
            "SystemDrive",
            "ComSpec",
            "PATHEXT",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
            "LOCALAPPDATA",
            "APPDATA",
            "USERPROFILE",
        ] {
            if let Some(value) = std::env::var_os(name) {
                map.insert(name.to_owned(), value.to_str().unwrap().to_owned());
            }
        }
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
        map.insert("PATH".to_owned(), format!(r"{root}\System32;{root}"));
        map.insert("SystemRoot".to_owned(), root);
    } else {
        for name in ["http_proxy", "https_proxy", "all_proxy"] {
            map.insert(name.to_owned(), PROXY.to_owned());
        }
        let socket_dir = scratch.socket_dir().to_str().unwrap().to_owned();
        map.insert("AGENT_BROWSER_SOCKET_DIR".to_owned(), socket_dir);
        map.insert("HOME".to_owned(), under("home"));
        map.insert("TMPDIR".to_owned(), under("tmp"));
        map.insert("LANG".to_owned(), "en_IN.UTF-8".to_owned());
        map.insert(
            "PATH".to_owned(),
            "/usr/bin:/bin:/usr/sbin:/sbin".to_owned(),
        );
    }
    map
}

/// Variables Windows itself puts in a new process's environment whatever block it is given:
/// on ARM64 the child gets `PROCESSOR_ARCHITECTURE`. The guard never passes them: the test sets
/// a hostile value in the guard's env, and the child must not see it.
const OS_SET: &[&str] = &["PROCESSOR_ARCHITECTURE"];

/// The value the test gives `OS_SET`'s variables in the guard's env.
const OS_SET_HOSTILE: &str = "clauseo-hostile";

#[test]
fn the_child_sees_exactly_the_allow_list_and_the_given_folder() {
    let scratch = Scratch::new("env");
    let args = ["mcp", "--tools", "core", "two words", "--x=y", "é"].map(OsStr::new);
    let mut command = guard_command(&scratch, &example("env_stub"), &args);
    command.envs(HOSTILE.iter().copied());
    if cfg!(windows) {
        command.env("PROCESSOR_ARCHITECTURE", OS_SET_HOSTILE);
    } else {
        command.env("LANG", "en_IN.UTF-8").env("lang", "xx");
    }
    let mut guard = Guard::start(command);
    let report = guard.read();
    let (status, rest) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_eq!(rest, "");

    let env: BTreeMap<String, String> = report["env"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()))
        .collect();
    let mut env = env;
    if cfg!(windows) {
        for name in OS_SET {
            if let Some(value) = env.remove(*name) {
                assert_ne!(
                    value, OS_SET_HOSTILE,
                    "{name} came through from the guard's env"
                );
            }
        }
    }
    assert_eq!(env, expected(&scratch));
    let args: Vec<&str> = report["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap())
        .collect();
    assert_eq!(args, ["mcp", "--tools", "core", "two words", "--x=y", "é"]);
    let cwd = report["cwd"].as_str().unwrap();
    assert_eq!(
        write::resolved(Path::new(cwd)).unwrap(),
        write::resolved(scratch.cwd()).unwrap()
    );
    let folders: &[&str] = if cfg!(windows) {
        &["downloads", "run", "screenshots", "tmp"]
    } else {
        &["downloads", "screenshots", "home", "tmp"]
    };
    for folder in folders {
        assert!(scratch.cwd().join(folder).is_dir(), "{folder}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert!(
            !scratch.cwd().join("run").exists(),
            "Unix sockets live under /tmp"
        );
        let metadata = std::fs::symlink_metadata(scratch.socket_dir()).unwrap();
        assert!(metadata.is_dir());
        assert_eq!(metadata.mode() & 0o7777, 0o700);
    }
    assert_eq!(report["env"].get("AI_GATEWAY_API_KEY"), None::<&Value>);
}

#[test]
fn folders_that_exist_are_kept() {
    let scratch = Scratch::new("env_again");
    write::create_dir_all(scratch.cwd().join("downloads")).unwrap();
    write::write(scratch.cwd().join("downloads").join("kept.pdf"), "x").unwrap();
    let args: [&OsStr; 0] = [];
    let mut guard = Guard::start(guard_command(&scratch, &example("env_stub"), &args));
    guard.read();
    assert_eq!(guard.finish().0.code(), Some(0));
    assert!(scratch.cwd().join("downloads").join("kept.pdf").is_file());
}
