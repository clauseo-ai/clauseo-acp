//! Tests of how a download sends `GH_TOKEN`: only to GitHub's API, only through a 0600 header
//! file that is gone when curl ends, never in curl's arguments or a failure message.

use super::*;
use crate::scratch::Scratch;

/// A token-shaped value no test command may ever show.
const TOKEN: &str = "ghs_TestTokenNeverInArgv0123456789";
const API_URL: &str = "https://api.github.com/repos/jgm/pandoc/releases/tags/3.13";

/// The rate limit's answer, as curl's `-f` reports it.
fn forbidden() -> Failure {
    Failure::new(
        "curl: (22) The requested URL returned error: 403",
        "try again",
    )
}

/// Asserts `cmd` shows no token, in its arguments or in the form failure messages print.
fn assert_no_token(cmd: &Cmd) {
    assert!(cmd.args.iter().all(|arg| !arg.contains(TOKEN)), "{cmd}");
    assert!(!cmd.to_string().contains(TOKEN), "{cmd}");
    assert!(cmd.env.is_empty(), "{cmd}");
    assert!(
        !cmd.args.iter().any(|arg| arg == "--location-trusted"),
        "{cmd}"
    );
}

#[test]
fn the_token_goes_to_githubs_api_alone() {
    assert_eq!(api_token(API_URL, Some(TOKEN)), Some(TOKEN));
    for url in [
        "https://github.com/jgm/pandoc/archive/refs/tags/3.13.tar.gz",
        "https://objects.githubusercontent.com/x",
        "https://pypi.org/pypi/lxml/6.0.0/json",
        "https://api.github.com.example/repos/x",
        "https://api.github.com@example.com/repos/x",
        "http://api.github.com/repos/x",
        "https://api.github.com",
    ] {
        assert_eq!(api_token(url, Some(TOKEN)), None, "{url}");
    }
    assert_eq!(
        api_token(API_URL, Some("")),
        None,
        "an empty token is no token"
    );
    assert_eq!(api_token(API_URL, None), None);
}

#[test]
fn an_api_request_reads_the_token_from_a_private_file_that_is_removed_after() {
    let dir = Scratch::new("net-token-api");
    let mut seen = None;
    let path = fetch_with(&dir, "release.json", API_URL, Some(TOKEN), |cmd| {
        assert_no_token(cmd);
        let at = cmd
            .args
            .iter()
            .position(|arg| arg == "-H")
            .expect("a header");
        assert_eq!(
            cmd.args.get(at + 1).map(String::as_str),
            Some("@.github-api-auth-header")
        );
        assert_eq!(cmd.args.last().map(String::as_str), Some(API_URL));
        let file = dir.join(AUTH_HEADER);
        seen = Some(std::fs::read_to_string(&file).expect("the header file is there"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).expect("stat").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        Ok(String::new())
    })
    .expect("fetched");
    assert_eq!(path, dir.join("release.json"));
    assert_eq!(
        seen.as_deref(),
        Some(&*format!("Authorization: Bearer {TOKEN}\n"))
    );
    assert!(!dir.join(AUTH_HEADER).exists(), "removed after curl");
}

#[test]
fn a_failed_api_request_fails_removes_the_file_and_never_quotes_the_token() {
    let dir = Scratch::new("net-token-403");
    let failure = fetch_with(&dir, "release.json", API_URL, Some(TOKEN), |cmd| {
        assert!(dir.join(AUTH_HEADER).exists());
        run_failure(cmd)
    })
    .expect_err("a 403 fails the fetch");
    assert!(failure.message.contains("403"), "{}", failure.message);
    assert!(!failure.message.contains(TOKEN) && !failure.fix.contains(TOKEN));
    assert!(
        !dir.join(AUTH_HEADER).exists(),
        "removed after a failed curl too"
    );
}

/// The failure `run` makes of a curl that failed: it quotes the command.
fn run_failure(cmd: &Cmd) -> Result<String, Failure> {
    Err(Failure::new(
        format!("`{cmd}` said: {}", forbidden().message),
        "try again",
    ))
}

#[test]
fn other_hosts_and_no_token_go_unauthenticated_as_before() {
    let dir = Scratch::new("net-token-none");
    let cases = [
        ("https://pypi.org/pypi/lxml/6.0.0/json", Some(TOKEN)),
        (API_URL, None),
        (API_URL, Some("")),
    ];
    for (url, token) in cases {
        let path = fetch_with(&dir, "x.json", url, token, |cmd| {
            assert_no_token(cmd);
            assert_eq!(cmd, &curl("x.json", url, None));
            assert!(!cmd.args.iter().any(|arg| arg == "-H"), "{cmd}");
            assert!(!dir.join(AUTH_HEADER).exists());
            Ok(String::new())
        })
        .expect("fetched");
        assert_eq!(path, dir.join("x.json"));
    }
    let failure = fetch_with(&dir, "x.json", API_URL, None, run_failure).expect_err("fails");
    assert!(
        failure.message.contains("403"),
        "a rate limit fails without a token too"
    );
}

#[test]
fn a_token_no_header_can_carry_is_refused_without_quoting_it() {
    let dir = Scratch::new("net-token-bad");
    for token in ["ghs_a\nX-Evil: 1", "ghs_a b", "ghs_\u{e9}"] {
        let failure = fetch_with(&dir, "x.json", API_URL, Some(token), |_| {
            panic!("curl must not run")
        })
        .expect_err("refused");
        assert_eq!(failure, bad_token());
        assert!(!failure.message.contains("ghs_") && !failure.fix.contains("ghs_"));
        assert!(!dir.join(AUTH_HEADER).exists());
    }
}

#[test]
fn a_header_file_a_killed_run_left_is_replaced() {
    let dir = Scratch::new("net-token-stale");
    Scratch::write(dir.join(AUTH_HEADER), "Authorization: Bearer old\n").expect("left behind");
    fetch_with(&dir, "x.json", API_URL, Some(TOKEN), |_| {
        let text = std::fs::read_to_string(dir.join(AUTH_HEADER)).expect("read");
        assert_eq!(text, format!("Authorization: Bearer {TOKEN}\n"));
        Ok(String::new())
    })
    .expect("fetched");
    assert!(!dir.join(AUTH_HEADER).exists());
}
