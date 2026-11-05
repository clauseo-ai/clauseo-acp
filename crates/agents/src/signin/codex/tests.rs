use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;

use super::device::{DEVICE_URL, DeviceLines, is_code, is_device_code_off, is_port_busy};
use super::identity::app_server_args;
use super::port_free;
use crate::signin::env::RunKind;

fn hosts() -> Vec<String> {
    vec!["auth.openai.com".to_owned(), "chatgpt.com".to_owned()]
}

fn read_all(lines: &[&str]) -> Vec<(String, String)> {
    let mut device = DeviceLines::default();
    lines
        .iter()
        .filter_map(|line| device.feed(line, &hosts()))
        .collect()
}

fn found(code: &str) -> Vec<(String, String)> {
    vec![(DEVICE_URL.to_owned(), code.to_owned())]
}

#[test]
fn the_device_url_and_code_are_read_from_codex_s_coloured_prompt() {
    // Codex 0.162.1's prompt, colours included.
    let lines = [
        "Follow these steps to sign in with ChatGPT using device code authorization:",
        "1. Open this link in your browser and sign in to your account",
        "   \u{1b}[94mhttps://auth.openai.com/codex/device\u{1b}[0m",
        "2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)\u{1b}[0m",
        "   \u{1b}[94mAB12-CD34\u{1b}[0m",
    ];
    assert_eq!(read_all(&lines), found("AB12-CD34"));
}

#[test]
fn the_url_and_code_on_one_line_are_read_once() {
    let line = "To sign in, open https://auth.openai.com/codex/device and enter the code ABCD-EFGH";
    assert_eq!(read_all(&[line, line]), found("ABCD-EFGH"));
}

#[test]
fn a_code_counts_only_after_the_device_url() {
    let lines = [
        "ABCD-EFGH",
        "https://auth.openai.com/codex/device",
        "then WXYZ-2345.",
    ];
    assert_eq!(read_all(&lines), found("WXYZ-2345"));
}

#[test]
fn any_other_address_is_never_the_device_page() {
    for url in [
        "http://auth.openai.com/codex/device",
        "https://auth.openai.com.evil.example/codex/device",
        "https://evil.example/codex/device",
        "https://user@auth.openai.com/codex/device",
        "https://auth.openai.com:8443/codex/device",
        "https://auth.openai.com/codex/device?next=https://evil.example",
        "https://auth.openai.com/codex/devices",
        "https://auth.openai.com/other",
    ] {
        assert_eq!(read_all(&[url, "ABCD-EFGH"]), [], "{url}");
    }
    // The snapshot's hosts must name it too.
    let mut device = DeviceLines::default();
    assert_eq!(device.feed(DEVICE_URL, &["chatgpt.com".to_owned()]), None);
    assert_eq!(device.feed("ABCD-EFGH", &["chatgpt.com".to_owned()]), None);
}

#[test]
fn a_code_is_four_capitals_or_digits_a_dash_and_four() {
    for good in ["ABCD-EFGH", "AB12-CD34", "0000-9999"] {
        assert!(is_code(good), "{good}");
    }
    for bad in [
        "abcd-efgh",
        "ABCD-EFG",
        "ABCDE-FGH",
        "ABCD_EFGH",
        "ABCD-EFGHI",
        "ABCD-EFG!",
        "ÄBCD-EFGH",
        "",
    ] {
        assert!(!is_code(bad), "{bad}");
    }
}

#[test]
fn codex_s_refusals_are_recognised() {
    assert!(is_device_code_off(
        "Enable device code sign-in for Codex, Excel, PowerPoint, and Word in ChatGPT Security Settings, then run codex login --device-auth again."
    ));
    assert!(!is_device_code_off("Successfully logged in"));
    assert!(is_port_busy(
        "Port 1455 is already in use: another login is listening on 127.0.0.1:1455."
    ));
    assert!(!is_port_busy(
        "Starting local login server on http://127.0.0.1:1455/auth/callback"
    ));
}

#[test]
fn a_held_port_is_not_free_and_a_released_one_is() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert!(!port_free(port));
    drop(listener);
    assert!(port_free(port));
}

#[test]
fn the_app_server_runs_with_the_file_store_and_the_overrides() {
    let args = app_server_args(RunKind::Own {
        home: Path::new("/data/agents/codex-acp/home"),
    });
    let args: Vec<&str> = args.iter().map(|arg| arg.to_str().unwrap()).collect();
    assert_eq!(args[..2], ["-c", "cli_auth_credentials_store=\"file\""]);
    assert!(args.contains(&"features.plugins=false"));
    assert_eq!(args.last(), Some(&"app-server"));
    assert_eq!(args.len(), 2 + 12 + 1);
}
