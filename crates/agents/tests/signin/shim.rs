//! The run's environment, and on macOS and Linux the `BROWSER` shim: what the child sees, which
//! address becomes `OpenUrl`, and the URL file's mode and end.

use std::ffi::OsStr;

use clauseo_agents::{Method, SigninCode, SigninState};

use crate::support::{Fixture, REMOTE_HINTS, claude, now};

fn cancelled() -> SigninState {
    SigninState::Failed {
        code: SigninCode::Cancelled,
        at: now(),
    }
}

#[test]
fn the_remote_hints_in_the_base_environment_never_reach_the_child() {
    let fx = Fixture::new("signin-env", "never");
    for hint in REMOTE_HINTS {
        assert!(fx.config.signin.base_env.contains_key(OsStr::new(hint)));
    }
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    events.until(|state| matches!(state, SigninState::Waiting { .. }));
    crate::support::eventually("the login wrote its names", || {
        fx.data.path().join("env-names").is_file()
    });
    // Only the login has run (no poll was let through), so these are its names.
    let names = fx.env_names();
    for hint in REMOTE_HINTS {
        assert!(
            !names.iter().any(|name| name == hint),
            "{hint} in {names:?}"
        );
    }
    for name in [
        "CLAUDE_CONFIG_DIR",
        "ANTHROPIC_API_KEY",
        "HOME",
        "FAKE_SIGNIN",
    ] {
        assert!(
            names.iter().any(|seen| seen == name),
            "{name} not in {names:?}"
        );
    }
    assert!(
        !names
            .iter()
            .any(|name| name == "CLAUDE_SECURESTORAGE_CONFIG_DIR")
    );
    #[cfg(unix)]
    for name in ["BROWSER", "CLAUSEO_SIGNIN_URL_FILE"] {
        assert!(
            names.iter().any(|seen| seen == name),
            "{name} not in {names:?}"
        );
    }
    #[cfg(windows)]
    assert!(!names.iter().any(|name| name == "CLAUSEO_SIGNIN_URL_FILE"));
    agents.cancel_sign_in(&claude()).unwrap();
    assert_eq!(events.until_end().1, cancelled());
}

#[cfg(unix)]
mod unix {
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    use clauseo_agents::{AgentsEvent, Method, SigninState};

    use super::cancelled;
    use crate::support::{BROWSER_URL, Fixture, POLL, PRINTED_URL, Seen, claude};

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_shim_captured_address_opens_once_and_a_printed_one_never() {
        let fx = Fixture::new("signin-open", "never");
        let (agents, events) = fx.start();
        agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
        let mut seen = events.until(|state| matches!(state, SigninState::Waiting { .. }));
        // The fake calls `$BROWSER` with one address, then prints the other on stdout.
        loop {
            let next = events.next();
            let open = matches!(next, Seen::Open(_));
            seen.push(next);
            if open {
                break;
            }
        }
        fx.timer.fire(POLL);
        fx.timer.fire(POLL);
        agents.cancel_sign_in(&claude()).unwrap();
        let (rest, last) = events.until_end();
        assert_eq!(last, cancelled());
        seen.extend(rest);
        let mut opened: Vec<String> = seen
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::Open(url) => Some(url),
                Seen::State(_) => None,
            })
            .collect();
        opened.extend(events.drain().into_iter().filter_map(|event| match event {
            AgentsEvent::OpenUrl { url, .. } => Some(url),
            _ => None,
        }));
        assert_eq!(opened, [BROWSER_URL]);
        assert!(!opened.iter().any(|url| url == PRINTED_URL));
    }

    #[test]
    fn the_url_file_is_private_and_gone_when_the_run_ends() {
        let fx = Fixture::new("signin-url-file", "never");
        let (agents, events) = fx.start();
        agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
        events.until(|state| matches!(state, SigninState::Waiting { .. }));
        let files = fx.url_files();
        assert_eq!(files.len(), 1, "{files:?}");
        assert_eq!(mode(&files[0]), 0o600);
        let shim = fx.signin_folder().join("browser");
        assert_eq!(mode(&shim), 0o700);
        crate::support::eventually("the login started", || !fx.runs_of("login").is_empty());
        let login = fx.runs_of("login").remove(0);
        assert_eq!(
            login.env.get(OsStr::new("CLAUSEO_SIGNIN_URL_FILE")),
            Some(&files[0].as_os_str().to_owned())
        );
        assert_eq!(
            login.env.get(OsStr::new("BROWSER")),
            Some(&shim.as_os_str().to_owned())
        );
        // The shim has written the address into it once the fake's address is opened.
        while !matches!(events.next(), Seen::Open(_)) {}
        assert_eq!(mode(&files[0]), 0o600);
        agents.cancel_sign_in(&claude()).unwrap();
        assert_eq!(events.until_end().1, cancelled());
        assert_eq!(fx.url_files(), Vec::<std::path::PathBuf>::new());

        // A run that completes removes its file too.
        let fx = Fixture::new("signin-url-file-ok", "ok-after:0");
        let (agents, events) = fx.start();
        agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
        let (_, last) = events.until_end();
        assert!(matches!(last, SigninState::SignedIn { .. }), "{last:?}");
        assert_eq!(fx.url_files(), Vec::<std::path::PathBuf>::new());
    }
}
