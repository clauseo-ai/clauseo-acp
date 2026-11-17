use std::collections::BTreeMap;
use std::ffi::OsString;

use super::{MARK, Redactor, Secrets, line, remember};
use crate::browser_guard::devtools::base64;
use crate::browser_guard::proxy_auth::{self, Password};

const SECRET: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
/// An obviously fake proxy password.
const PASSWORD: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";

/// Remembers the fake password's two forms, as the guard's start does, and returns its
/// `Proxy-Authorization` value.
fn hide_password() -> String {
    proxy_auth::hide(&Password::parse(PASSWORD).unwrap());
    base64(format!("clauseo:{PASSWORD}").as_bytes())
}

fn url() -> String {
    format!("ws://127.0.0.1:50123/devtools/browser/{SECRET}")
}

fn secrets() -> Secrets {
    let env = BTreeMap::from([("AGENT_BROWSER_CDP".to_owned(), OsString::from(url()))]);
    Secrets::of(&env).unwrap()
}

fn mark() -> String {
    String::from_utf8(MARK.to_vec()).unwrap()
}

/// Everything the redactor gives out for `input` read `step` bytes at a time.
fn redacted(input: &str, step: usize) -> String {
    let mut redactor = Redactor::new(secrets());
    let mut out = Vec::new();
    for chunk in input.as_bytes().chunks(step) {
        out.extend(redactor.push(chunk));
    }
    out.extend(redactor.finish());
    String::from_utf8(out).unwrap()
}

#[test]
fn the_url_and_the_secret_never_reach_the_guards_stderr_however_the_reads_split() {
    let mark = mark();
    let input = format!(
        "error: cannot connect to {} (refused)\nsecret {SECRET} alone\nno secret here\n\
         ends on a URL: {}",
        url(),
        url()
    );
    let expected = format!(
        "error: cannot connect to {mark} (refused)\nsecret {mark} alone\nno secret here\n\
         ends on a URL: {mark}"
    );
    for step in [1, 2, 7, 63, 64, 65, 4096] {
        let out = redacted(&input, step);
        assert_eq!(out, expected, "reads of {step} bytes");
        assert!(!out.contains(&SECRET[..16]), "reads of {step} bytes");
    }
}

#[test]
fn the_secret_in_any_case_and_its_cut_short_start_are_taken_out_too() {
    let mark = mark();
    let upper = SECRET.to_ascii_uppercase();
    let url_upper = url().to_ascii_uppercase();
    let cut = &SECRET[..16];
    let short = &SECRET[..15];
    let input = format!(
        "A {upper} B\nC {url_upper}\nD browser/{cut}\u{2026}\nE x{} y\nF {short} kept\n\
         G ab{SECRET}cd\n",
        &SECRET[..40]
    );
    let expected = format!(
        "A {mark} B\nC {mark}\nD browser/{mark}\u{2026}\nE x{mark} y\nF {short} kept\n\
         G ab{mark}cd\n"
    );
    for step in [1, 3, 16, 17, 4096] {
        assert_eq!(redacted(&input, step), expected, "reads of {step} bytes");
    }
}

#[test]
fn a_line_goes_out_when_it_ends_and_text_without_a_url_is_unchanged() {
    let mut redactor = Redactor::new(secrets());
    assert_eq!(redactor.push(b"warn: slow\n"), b"warn: slow\n");
    // Hex that is not the secret's start goes out once it cannot be.
    assert_eq!(redactor.push(b"id 0012"), b"id 0012");
    // A one-shot agent-browser has no URL: nothing to take out.
    assert_eq!(Secrets::of(&BTreeMap::new()), None);
}

/// agent-browser's MCP replies: a line with the secret stays the same JSON with the mark in
/// its place; a line without one, invalid UTF-8 included, is passed on as it came.
#[test]
fn stdout_lines_keep_their_json_and_their_bytes() {
    remember(&secrets());
    let reply = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "result": {"content": [{"type": "text", "text": format!(
            "connected to {} ({}...)", url(), &SECRET[..20].to_ascii_uppercase()
        )}]},
    })
    .to_string();
    let out = line(reply.as_bytes()).expect("the line held the secret");
    let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        parsed["result"]["content"][0]["text"],
        format!("connected to {} ({}...)", mark(), mark())
    );
    assert!(
        !String::from_utf8_lossy(&out)
            .to_ascii_lowercase()
            .contains(&SECRET[..16])
    );
    let plain: &[u8] =
        b"{\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{\"text\":\"00112233445566 \xff\"}}";
    assert_eq!(line(plain), None);
}

#[test]
fn the_password_is_hidden_on_stdout_on_stderr_and_in_the_guards_notes() {
    hide_password();
    let mark = mark();
    let text = format!(
        "proxy said {PASSWORD} and {}",
        PASSWORD.to_ascii_uppercase()
    );
    let expected = format!("proxy said {mark} and {mark}");
    let out = line(text.as_bytes()).expect("the line held the password");
    assert_eq!(String::from_utf8(out).unwrap(), expected);
    assert_eq!(
        redacted(&format!("{text}\n"), 4096),
        format!("{expected}\n")
    );
    assert_eq!(super::redacted(&text), expected);
}

#[test]
fn the_proxy_authorization_value_is_hidden_whole_however_the_reads_split() {
    let basic = hide_password();
    let mark = mark();
    let header = format!("Proxy-Authorization: Basic {basic}\nnext\n");
    for step in [1, 5, 16, 95, 96, 97, 4096] {
        assert_eq!(
            redacted(&header, step),
            format!("Proxy-Authorization: Basic {mark}\nnext\n"),
            "reads of {step} bytes"
        );
    }
    let reply = serde_json::json!({"headers": {"Proxy-Authorization": format!("Basic {basic}")}})
        .to_string();
    let out = String::from_utf8(line(reply.as_bytes()).unwrap()).unwrap();
    assert!(out.contains(&format!("Basic {mark}")) && !out.contains(&basic));
}

/// Every Basic value for the user `clauseo` starts the same 12 characters, so a start of
/// the credential is taken out only past them: another value sharing its first 16 (and so
/// the password's first 4 hex digits) is left as it is, and a cut-short real one is not.
#[test]
fn another_basic_value_with_the_same_start_is_not_hidden() {
    let basic = hide_password();
    let mark = mark();
    let other = base64(format!("clauseo:feed{}", &SECRET[..60]).as_bytes());
    assert_eq!(other[..16], basic[..16]);
    assert_ne!(other[..17], basic[..17]);
    let header = format!("Proxy-Authorization: Basic {other}\n");
    assert_eq!(line(header.as_bytes()), None);
    for step in [1, 16, 17, 4096] {
        assert_eq!(redacted(&header, step), header, "reads of {step} bytes");
    }
    // 27 characters: the user's 12 and 15 more, too short to tell.
    let short = format!("Basic {}...", &basic[..27]);
    assert_eq!(line(short.as_bytes()), None);
    let cut = format!("Basic {}...", &basic[..28]);
    assert_eq!(
        String::from_utf8(line(cut.as_bytes()).unwrap()).unwrap(),
        format!("Basic {mark}...")
    );
}

#[test]
fn the_password_split_across_reads_is_still_hidden() {
    hide_password();
    let mark = mark();
    let input = format!("a {PASSWORD} b\nc {}\u{2026}\n", &PASSWORD[..20]);
    for step in [1, 2, 7, 16, 63, 64, 65] {
        let out = redacted(&input, step);
        assert_eq!(
            out,
            format!("a {mark} b\nc {mark}\u{2026}\n"),
            "reads of {step} bytes"
        );
        assert!(!out.contains(&PASSWORD[..16]), "reads of {step} bytes");
    }
}

#[test]
fn a_literal_is_taken_out_with_a_long_enough_start() {
    let hex = Secrets::literal(SECRET.as_bytes());
    assert_eq!(
        super::scan(&SECRET.as_bytes()[..16], std::slice::from_ref(&hex), false).1,
        16
    );
    let found = super::scan(&SECRET.as_bytes()[..16], &[hex], false).0;
    assert_eq!(found, Some(MARK.to_vec()));
    let later = Secrets::literal(SECRET.as_bytes()).start_after(40);
    assert_eq!(
        super::scan(
            &SECRET.as_bytes()[..55],
            std::slice::from_ref(&later),
            false
        )
        .0,
        None
    );
    let found = super::scan(
        &SECRET.as_bytes()[..56],
        std::slice::from_ref(&later),
        false,
    )
    .0;
    assert_eq!(found, Some(MARK.to_vec()));
    assert_eq!(
        super::scan(SECRET.as_bytes(), &[later], false).0,
        Some(MARK.to_vec())
    );
    // An empty literal matches nothing.
    assert_eq!(super::scan(b"abc", &[Secrets::literal(b"")], false).0, None);
}

/// The session key (Windows) is taken out as the secret is, from stderr and from stdout.
#[test]
fn the_session_key_is_taken_out_wherever_it_appears() {
    const KEY: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";
    let env = BTreeMap::from([
        ("AGENT_BROWSER_CDP".to_owned(), OsString::from(url())),
        ("AGENT_BROWSER_SESSION_KEY".to_owned(), OsString::from(KEY)),
    ]);
    let with_key = Secrets::of(&env).unwrap();
    let mark = mark();
    let input = format!(
        "key {KEY}\nupper {}\ncut {}\nsecret {SECRET}\n",
        KEY.to_ascii_uppercase(),
        &KEY[..20]
    );
    let expected = format!("key {mark}\nupper {mark}\ncut {mark}\nsecret {mark}\n");
    for step in [1, 5, 64, 4096] {
        let mut redactor = Redactor::new(with_key.clone());
        let mut out = Vec::new();
        for chunk in input.as_bytes().chunks(step) {
            out.extend(redactor.push(chunk));
        }
        out.extend(redactor.finish());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            expected,
            "reads of {step} bytes"
        );
    }
    remember(&with_key);
    let reply = format!("{{\"text\":\"{KEY}\"}}");
    let out = line(reply.as_bytes()).expect("the line held the key");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!("{{\"text\":\"{mark}\"}}")
    );
}

/// The session key (Windows) is its own entry: its start is taken out at [`MIN_PREFIX`]
/// characters, and a shorter start is no secret.
#[test]
fn a_session_key_start_is_taken_out_at_min_prefix() {
    const KEY: &str = "00ffeeddccbbaa998877665544332211ffeeddccbbaa998877665544332211";
    let env = BTreeMap::from([
        ("AGENT_BROWSER_CDP".to_owned(), OsString::from(url())),
        ("AGENT_BROWSER_SESSION_KEY".to_owned(), OsString::from(KEY)),
    ]);
    let with_key = Secrets::of(&env).unwrap();
    let mark = mark();
    let input = format!("cut {}\nshort {} kept\n", &KEY[..16], &KEY[..15]);
    let expected = format!("cut {mark}\nshort {} kept\n", &KEY[..15]);
    for step in [1, 3, 16, 4096] {
        let mut redactor = Redactor::new(with_key.clone());
        let mut out = Vec::new();
        for chunk in input.as_bytes().chunks(step) {
            out.extend(redactor.push(chunk));
        }
        out.extend(redactor.finish());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            expected,
            "reads of {step} bytes"
        );
    }
}
