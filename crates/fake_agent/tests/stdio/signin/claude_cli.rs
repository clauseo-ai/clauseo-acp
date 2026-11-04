//! Claude's `claude-cli` mode over real processes: the login, status, logout, the browser,
//! paste, cancel, never, and the Claude home folder.

use clauseo_fake_agent::EXIT_OK;
use serde_json::json;

use super::{
    CLAUDE_LOGOUT, CLAUDE_MANUAL_LINE, CLAUDE_PROMPT, CLAUDE_STATUS_OUT, claude, claude_dir,
    claude_status, home, marker, read_marker, text,
};
use crate::harness::{Finished, Scratch};
#[cfg(unix)]
use crate::write;

#[test]
fn claude_login_prints_its_lines_and_plants_the_marker_in_the_claude_dir() {
    let scratch = Scratch::new("claude_login_prints_its_lines");
    let home = home(&scratch);
    let finished = claude(
        &scratch,
        &home,
        &["--cli", "auth", "login", "--claudeai"],
        &[],
    )
    .finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert_eq!(
        finished.stdout,
        format!("{CLAUDE_MANUAL_LINE}\n{CLAUDE_PROMPT}\nLogin successful.\n")
    );
    assert_eq!(
        read_marker(&claude_dir(&home)),
        json!({"authMethod": "claude.ai", "email": "test@example.com"})
    );
}

#[test]
fn claude_accepts_the_bare_form_and_the_cli_form_with_either_flag() {
    let scratch = Scratch::new("claude_accepts_the_forms");
    let home = home(&scratch);
    let forms: [&[&str]; 4] = [
        &["auth", "login"],
        &["auth", "login", "--claudeai"],
        &["--cli", "auth", "login", "--console"],
        &["--cli", "auth", "status", "--json"],
    ];
    for args in forms {
        let finished = claude(&scratch, &home, args, &[]).finish();
        assert_eq!(
            finished.status.code(),
            Some(EXIT_OK),
            "{args:?}: {}",
            finished.stderr
        );
    }
}

#[test]
fn claude_status_flips_after_login_and_back_after_logout() {
    let scratch = Scratch::new("claude_status_flips");
    let home = home(&scratch);
    let claude_dir = claude_dir(&home);
    assert_eq!(
        claude_status(&scratch, &home, &[]),
        format!("{CLAUDE_STATUS_OUT}\n")
    );
    let login = claude(&scratch, &home, &["auth", "login"], &[]).finish();
    assert_eq!(login.status.code(), Some(EXIT_OK), "{}", login.stderr);
    assert_eq!(
        claude_status(&scratch, &home, &[]),
        format!(
            "{}\n",
            r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","email":"test@example.com","subscriptionType":"max"}"#
        )
    );
    let logout = claude(&scratch, &home, &["auth", "logout"], &[]).finish();
    assert_eq!(logout.status.code(), Some(EXIT_OK), "{}", logout.stderr);
    assert_eq!(logout.stdout, format!("{CLAUDE_LOGOUT}\n"));
    assert!(!marker(&claude_dir).exists());
    assert_eq!(
        claude_status(&scratch, &home, &[]),
        format!("{CLAUDE_STATUS_OUT}\n")
    );
}

#[cfg(unix)]
#[test]
fn claude_login_runs_browser_with_the_url_and_ignores_its_exit_code() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("claude_login_runs_browser");
    let home = home(&scratch);
    let log = scratch.path().join("browser.log");
    let browser = scratch.path().join("browser.sh");
    write::write(
        &browser,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\nexit 3\n",
            text(&log)
        ),
    )
    .unwrap();
    write::set_permissions(&browser, std::fs::Permissions::from_mode(0o755)).unwrap();
    let finished = claude(
        &scratch,
        &home,
        &["auth", "login"],
        &[("BROWSER", text(&browser))],
    )
    .finish();
    // The browser exits 3; the login still finishes with 0.
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "https://claude.com/cai/oauth/authorize?fake=1\n"
    );
    assert_eq!(
        finished.stdout,
        format!("{CLAUDE_MANUAL_LINE}\n{CLAUDE_PROMPT}\nLogin successful.\n")
    );
}

#[test]
fn claude_cancel_prints_access_denied_and_exits_1() {
    let scratch = Scratch::new("claude_cancel");
    let home = home(&scratch);
    let finished = claude(
        &scratch,
        &home,
        &["auth", "login"],
        &[("FAKE_SIGNIN", "cancel")],
    )
    .finish();
    assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
    assert_eq!(
        finished.stdout,
        format!("{CLAUDE_MANUAL_LINE}\n{CLAUDE_PROMPT}\nOAuth error: access_denied\n")
    );
    assert!(!marker(&claude_dir(&home)).exists());
}

/// Runs `auth login` under `paste`, sends `code` (or closes stdin for `None`) after the two
/// prompt lines, and returns what the run left.
fn paste(scratch: &Scratch, home: &str, code: Option<&str>) -> Finished {
    let mut agent = claude(
        scratch,
        home,
        &["auth", "login"],
        &[("FAKE_SIGNIN", "paste")],
    );
    assert_eq!(agent.read_raw(), CLAUDE_MANUAL_LINE);
    assert_eq!(agent.read_raw(), CLAUDE_PROMPT);
    match code {
        Some(code) => agent.send_raw(code),
        None => agent.close_stdin(),
    }
    agent.finish()
}

#[test]
fn claude_paste_accepts_only_fake_code() {
    let scratch = Scratch::new("claude_paste");
    let home = home(&scratch);
    let claude_dir = claude_dir(&home);
    for code in [
        Some("fake-code"),
        Some("FAKE-CODE "),
        Some("FAKE-CODE-2"),
        Some(""),
        None,
    ] {
        let finished = paste(&scratch, &home, code);
        assert_eq!(
            finished.status.code(),
            Some(1),
            "{code:?}: {}",
            finished.stderr
        );
        assert_eq!(
            finished.stdout, "Login failed: the code is not FAKE-CODE.\n",
            "{code:?}"
        );
        assert!(!marker(&claude_dir).exists(), "{code:?} planted a marker");
    }
    let finished = paste(&scratch, &home, Some("FAKE-CODE"));
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert_eq!(finished.stdout, "Login successful.\n");
    assert!(marker(&claude_dir).is_file());
}

#[test]
fn claude_never_waits_until_it_is_killed() {
    let scratch = Scratch::new("claude_never");
    let home = home(&scratch);
    let mut agent = claude(
        &scratch,
        &home,
        &["auth", "login"],
        &[("FAKE_SIGNIN", "never")],
    );
    assert_eq!(agent.read_raw(), CLAUDE_MANUAL_LINE);
    assert_eq!(agent.read_raw(), CLAUDE_PROMPT);
    assert!(agent.running());
    agent.kill();
    let finished = agent.finish();
    assert!(!finished.status.success());
    assert_eq!(finished.stdout, "");
    assert!(!marker(&claude_dir(&home)).exists());
}

#[test]
fn ok_after_writes_the_marker_then_exits_zero() {
    let scratch = Scratch::new("claude_ok_after");
    let home = home(&scratch);
    let finished = claude(
        &scratch,
        &home,
        &["auth", "login"],
        &[("FAKE_SIGNIN", "ok-after:50")],
    )
    .finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert_eq!(
        finished.stdout,
        format!("{CLAUDE_MANUAL_LINE}\n{CLAUDE_PROMPT}\nLogin successful.\n")
    );
    assert!(marker(&claude_dir(&home)).is_file());
}

#[test]
fn claude_home_is_the_securestorage_dir_when_it_is_set() {
    let scratch = Scratch::new("claude_home_securestorage");
    let home = home(&scratch);
    let secure = scratch.path().join("secure");
    let config = scratch.path().join("config");
    let finished = claude(
        &scratch,
        &home,
        &["auth", "login"],
        &[
            ("CLAUDE_SECURESTORAGE_CONFIG_DIR", text(&secure)),
            ("CLAUDE_CONFIG_DIR", text(&config)),
        ],
    )
    .finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert!(marker(&secure).is_file());
    assert!(!marker(&config).exists());
    assert!(!marker(&claude_dir(&home)).exists());
}

#[test]
fn claude_home_is_home_dot_claude_when_securestorage_is_empty() {
    let scratch = Scratch::new("claude_home_empty_securestorage");
    let home = home(&scratch);
    let config = scratch.path().join("config");
    let finished = claude(
        &scratch,
        &home,
        &["auth", "login"],
        &[
            ("CLAUDE_SECURESTORAGE_CONFIG_DIR", ""),
            ("CLAUDE_CONFIG_DIR", text(&config)),
        ],
    )
    .finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert!(marker(&claude_dir(&home)).is_file());
    assert!(!marker(&config).exists());
}

#[test]
fn claude_home_is_the_config_dir_when_securestorage_is_unset() {
    let scratch = Scratch::new("claude_home_config_dir");
    let home = home(&scratch);
    let config = scratch.path().join("config");
    let finished = claude(
        &scratch,
        &home,
        &["auth", "login"],
        &[("CLAUDE_CONFIG_DIR", text(&config))],
    )
    .finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert!(marker(&config).is_file());
    assert!(!marker(&claude_dir(&home)).exists());
}

#[test]
fn claude_home_is_home_dot_claude_when_nothing_is_set() {
    let scratch = Scratch::new("claude_home_default");
    let home = home(&scratch);
    let finished = claude(&scratch, &home, &["auth", "login"], &[]).finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert!(marker(&claude_dir(&home)).is_file());
}

#[test]
fn reuse_plants_the_claude_marker_for_any_subcommand() {
    let scratch = Scratch::new("reuse_claude");
    let home = home(&scratch);
    let status = claude_status(&scratch, &home, &[("FAKE_SIGNIN", "reuse:console")]);
    assert_eq!(
        status,
        format!(
            "{}\n",
            r#"{"loggedIn":true,"authMethod":"console","apiProvider":"firstParty","email":"test@example.com","subscriptionType":"max"}"#
        )
    );
    assert_eq!(
        read_marker(&claude_dir(&home)),
        json!({"authMethod": "console", "email": "test@example.com"})
    );
}

/// Status prints the marker's email, so a test can stand another account in the lawyer's home.
#[test]
fn claude_status_prints_the_marker_s_email() {
    let scratch = Scratch::new("claude_status_email");
    let home = home(&scratch);
    let dir = claude_dir(&home);
    write::create_dir_all(&dir).unwrap();
    write::write(
        marker(&dir),
        r#"{"authMethod":"claude.ai","email":"other@example.com"}"#,
    )
    .unwrap();
    let status: serde_json::Value =
        serde_json::from_str(claude_status(&scratch, &home, &[]).trim()).unwrap();
    assert_eq!(status["email"], "other@example.com");
}

/// `FAKE_SIGNIN=secure-ignored` stands for a Claude Code that dropped
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR`: a reused session's status looks in `CLAUDE_CONFIG_DIR`.
#[test]
fn secure_ignored_drops_the_secure_storage_variable() {
    let scratch = Scratch::new("claude_secure_ignored");
    let home = home(&scratch);
    let dir = claude_dir(&home);
    write::create_dir_all(&dir).unwrap();
    write::write(marker(&dir), r#"{"authMethod":"claude.ai"}"#).unwrap();
    let config = scratch.path().join("clauseo-home");
    let reused = [
        ("CLAUDE_CONFIG_DIR", text(&config)),
        ("CLAUDE_SECURESTORAGE_CONFIG_DIR", ""),
    ];
    let honoured = claude_status(&scratch, &home, &reused);
    assert!(honoured.contains(r#""loggedIn":true"#), "{honoured}");
    let mut ignored = reused.to_vec();
    ignored.push(("FAKE_SIGNIN", "secure-ignored"));
    assert_eq!(
        claude_status(&scratch, &home, &ignored),
        format!("{CLAUDE_STATUS_OUT}\n")
    );
}
