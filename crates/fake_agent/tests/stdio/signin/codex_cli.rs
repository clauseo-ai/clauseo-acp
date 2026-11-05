//! Codex's `codex.js` mode over real processes: the browser and device logins, `login status`,
//! `logout`, the port, the Codex home folder, and `app-server`.

use std::path::PathBuf;

use clauseo_fake_agent::EXIT_OK;
use serde_json::json;

use super::{
    CODEX_CALLBACK_LINE, CODEX_DEVICE_LINE, CODEX_REFUSAL, codex, codex_status, home, marker,
    read_marker, text,
};
use crate::harness::Scratch;

#[test]
fn reuse_plants_the_codex_marker_in_codex_home() {
    let scratch = Scratch::new("reuse_codex");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let env = [
        ("FAKE_SIGNIN", "reuse:chatgpt"),
        ("CODEX_HOME", text(&codex_home)),
    ];
    assert_eq!(
        codex_status(&scratch, &home, &env, 0),
        "Logged in using ChatGPT\n"
    );
    assert!(marker(&codex_home).is_file());
    assert!(!PathBuf::from(&home).join(".codex").exists());
}

/// S18's reuse tests stand a dummy file with Codex's login file name in `~/.codex` and the
/// engine hard-links it into `CODEX_HOME`: the fake takes that file as its marker.
#[test]
fn a_dummy_codex_login_file_counts_as_the_marker() {
    let scratch = Scratch::new("codex_login_file");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let env = [("CODEX_HOME", text(&codex_home))];
    assert_eq!(codex_status(&scratch, &home, &env, 1), "Not logged in\n");
    crate::write::create_dir_all(&codex_home).unwrap();
    crate::write::write(codex_home.join(concat!("auth", ".json")), "").unwrap();
    assert_eq!(
        codex_status(&scratch, &home, &env, 0),
        "Logged in using ChatGPT\n"
    );
}

#[test]
fn codex_login_prints_the_callback_line_and_writes_the_marker_in_codex_home() {
    let scratch = Scratch::new("codex_login");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let finished = codex(
        &scratch,
        &home,
        &["login"],
        &[("CODEX_HOME", text(&codex_home))],
    )
    .finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert_eq!(
        finished.stdout,
        format!("{CODEX_CALLBACK_LINE}\nSuccessfully logged in\n")
    );
    assert_eq!(read_marker(&codex_home)["email"], "test@example.com");
    assert!(!PathBuf::from(&home).join(".codex").exists());
}

#[test]
fn codex_home_is_home_dot_codex_when_codex_home_is_unset() {
    let scratch = Scratch::new("codex_home_default");
    let home = home(&scratch);
    let finished = codex(&scratch, &home, &["login"], &[]).finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert!(marker(&PathBuf::from(&home).join(".codex")).is_file());
}

#[test]
fn codex_login_status_flips_after_login_and_back_after_logout() {
    let scratch = Scratch::new("codex_status_flips");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let env = [("CODEX_HOME", text(&codex_home))];
    assert_eq!(codex_status(&scratch, &home, &env, 1), "Not logged in\n");
    let login = codex(&scratch, &home, &["login"], &env).finish();
    assert_eq!(login.status.code(), Some(EXIT_OK), "{}", login.stderr);
    assert_eq!(
        codex_status(&scratch, &home, &env, 0),
        "Logged in using ChatGPT\n"
    );
    let logout = codex(&scratch, &home, &["logout"], &env).finish();
    assert_eq!(logout.status.code(), Some(EXIT_OK), "{}", logout.stderr);
    assert_eq!(logout.stdout, "Successfully logged out\n");
    assert!(!marker(&codex_home).exists());
    assert_eq!(codex_status(&scratch, &home, &env, 1), "Not logged in\n");
}

#[test]
fn leading_config_overrides_are_skipped_before_the_command() {
    let scratch = Scratch::new("codex_overrides");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let env = [("CODEX_HOME", text(&codex_home))];
    let store = "cli_auth_credentials_store=\"file\"";
    let login = codex(&scratch, &home, &["-c", store, "login"], &env).finish();
    assert_eq!(login.status.code(), Some(EXIT_OK), "{}", login.stderr);
    assert!(marker(&codex_home).is_file());
    let status = codex(
        &scratch,
        &home,
        &[
            "-c",
            store,
            "-c",
            "features.plugins=false",
            "login",
            "status",
        ],
        &env,
    )
    .finish();
    assert_eq!(status.status.code(), Some(EXIT_OK), "{}", status.stderr);
    // An override after the command is still a usage error.
    let late = codex(&scratch, &home, &["login", "-c", store], &env).finish();
    assert_eq!(late.status.code(), Some(2), "{}", late.stderr);
}

#[test]
fn codex_device_auth_prints_the_url_and_the_code_then_logs_in() {
    let scratch = Scratch::new("codex_device");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let finished = codex(
        &scratch,
        &home,
        &["login", "--device-auth"],
        &[("CODEX_HOME", text(&codex_home))],
    )
    .finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert_eq!(
        finished.stdout,
        format!("{CODEX_DEVICE_LINE}\nSuccessfully logged in\n")
    );
    assert!(marker(&codex_home).is_file());
}

#[test]
fn codex_device_off_refuses_with_codexs_message_and_the_browser_login_still_works() {
    let scratch = Scratch::new("codex_device_off");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let env = [
        ("CODEX_HOME", text(&codex_home)),
        ("FAKE_SIGNIN", "device-off"),
    ];
    let refused = codex(&scratch, &home, &["login", "--device-auth"], &env).finish();
    assert_eq!(refused.status.code(), Some(1), "{}", refused.stderr);
    assert_eq!(refused.stdout, format!("{CODEX_REFUSAL}\n"));
    assert!(!marker(&codex_home).exists());
    let browser = codex(&scratch, &home, &["login"], &env).finish();
    assert_eq!(browser.status.code(), Some(EXIT_OK), "{}", browser.stderr);
    assert!(marker(&codex_home).is_file());
}

#[test]
fn codex_port_busy_prints_that_the_port_is_in_use_and_exits_1() {
    let scratch = Scratch::new("codex_port_busy");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let finished = codex(
        &scratch,
        &home,
        &["login"],
        &[
            ("CODEX_HOME", text(&codex_home)),
            ("FAKE_SIGNIN", "port-busy"),
        ],
    )
    .finish();
    assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
    assert_eq!(finished.stdout.lines().count(), 1, "{}", finished.stdout);
    assert!(finished.stdout.starts_with("Port 1455 is already in use"));
    assert!(!marker(&codex_home).exists());
}

#[test]
fn app_server_account_read_names_the_account_once_logged_in() {
    let scratch = Scratch::new("codex_app_server");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let login = codex(
        &scratch,
        &home,
        &["login"],
        &[("CODEX_HOME", text(&codex_home))],
    )
    .finish();
    assert_eq!(login.status.code(), Some(EXIT_OK), "{}", login.stderr);
    let mut agent = codex(
        &scratch,
        &home,
        &["app-server"],
        &[("CODEX_HOME", text(&codex_home))],
    );
    agent.send(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {"clientInfo": {"name": "test", "version": "0"}}}));
    let reply = agent.read_line();
    assert_eq!(reply["id"], 0, "{reply}");
    assert_eq!(reply["result"]["codexHome"], text(&codex_home));
    assert_eq!(reply["result"]["userAgent"], "fake_agent codex-cli");
    // A notification has no id, so it gets no reply: the next line read is account/read's.
    agent.send(&json!({"jsonrpc": "2.0", "method": "initialized"}));
    agent.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "account/read", "params": {}}));
    assert_eq!(
        agent.read_line(),
        json!({"jsonrpc": "2.0", "id": 1, "result": {
            "account": {"type": "chatgpt", "email": "test@example.com", "planType": "plus"},
            "requiresOpenaiAuth": false}})
    );
    agent.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "no/such"}));
    assert_eq!(agent.read_line()["error"]["code"], -32601);
    agent.expect_exit(EXIT_OK);
}

#[test]
fn app_server_account_read_is_signed_out_without_a_marker() {
    let scratch = Scratch::new("codex_app_server_signed_out");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    let mut agent = codex(
        &scratch,
        &home,
        &["app-server"],
        &[("CODEX_HOME", text(&codex_home))],
    );
    agent.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "account/read", "params": {}}));
    assert_eq!(
        agent.read_line(),
        json!({"jsonrpc": "2.0", "id": 1, "result": {
            "account": null, "requiresOpenaiAuth": true}})
    );
    agent.expect_exit(EXIT_OK);
    assert!(!marker(&codex_home).exists());
}

/// S18's race tests need a reusable login with no email: a dummy login file whose
/// `authMethod` is `apikey` makes `account/read` name an API-key account.
#[test]
fn app_server_account_read_gives_an_api_key_account_for_an_apikey_marker() {
    let scratch = Scratch::new("codex_app_server_apikey");
    let home = home(&scratch);
    let codex_home = scratch.path().join("codex-home");
    crate::write::create_dir_all(&codex_home).unwrap();
    crate::write::write(
        codex_home.join(concat!("auth", ".json")),
        r#"{"authMethod": "apikey"}"#,
    )
    .unwrap();
    let mut agent = codex(
        &scratch,
        &home,
        &["app-server"],
        &[("CODEX_HOME", text(&codex_home))],
    );
    agent.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "account/read", "params": {}}));
    assert_eq!(
        agent.read_line(),
        json!({"jsonrpc": "2.0", "id": 1, "result": {
            "account": {"type": "apiKey"}, "requiresOpenaiAuth": true}})
    );
    agent.expect_exit(EXIT_OK);
}
