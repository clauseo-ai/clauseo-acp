//! The process tests: the built mode over pipes and a loopback listener.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread;

use serde_json::Value;

use super::support::{OK, Scratch, TOKEN, closed_port, connector, run, start};

#[test]
fn a_bad_setting_exits_2_with_one_usage_line_and_no_token() {
    let scratch = Scratch::new("bad");
    let good = scratch.token_file(TOKEN);
    let upper = scratch.token_file(&TOKEN.to_uppercase());
    let cases = [
        (connector("127.0.0.1:61000", &good), "bad-relay-port"),
        (connector("localhost:61000", &good), "bad-relay-host"),
        (connector("127.0.0.1:61000", &upper), "bad-token"),
        (
            connector("127.0.0.1:61000", &scratch.missing()),
            "missing-token",
        ),
    ];
    for (mut command, case) in cases {
        let output = run(&mut command);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(2), "{case}");
        assert_eq!(stderr.lines().count(), 1, "{case}: {stderr:?}");
        assert!(
            stderr.starts_with("usage: clauseo-mcp connector <name>: "),
            "{case}: {stderr:?}"
        );
        assert_eq!(output.stdout, Vec::<u8>::new(), "{case}");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        for text in [&stderr, &stdout] {
            assert!(!text.contains(TOKEN), "{case}: the token is in the output");
            assert!(
                !text.contains(&TOKEN.to_uppercase()),
                "{case}: the token file's content is in the output"
            );
        }
    }
}

#[test]
fn the_process_relays_both_ways_and_exits_0_when_stdin_closes() {
    let scratch = Scratch::new("relay");
    let file = scratch.token_file(&format!("{TOKEN}\n"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let relay = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        let mut writer = &stream;
        writer.write_all(format!("{OK}\n").as_bytes()).unwrap();
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n")
            .unwrap();
        // The process half-closes once its stdin closes; the relay then sees the end.
        let mut rest = String::new();
        assert_eq!(reader.read_line(&mut rest).unwrap(), 0);
        (hello, request)
    });
    let mut command = connector(&format!("127.0.0.1:{port}"), &file);
    let mut child = start(&mut command);
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n")
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut reply = String::new();
    stdout.read_line(&mut reply).unwrap();
    assert_eq!(
        reply,
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n"
    );
    drop(stdin);
    let mut rest = String::new();
    assert_eq!(stdout.read_to_string(&mut rest).unwrap(), 0);
    assert!(child.wait().unwrap().success());
    let (hello, request) = relay.join().unwrap();
    assert_eq!(
        hello,
        format!("{{\"clauseo_relay\":1,\"token\":\"{TOKEN}\",\"connector\":\"case-law\"}}\n")
    );
    assert_eq!(
        request,
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n"
    );
}

#[test]
fn with_no_relay_a_request_gets_the_error_and_the_process_exits_0() {
    let scratch = Scratch::new("none");
    let file = scratch.token_file(TOKEN);
    let mut command = connector(&format!("127.0.0.1:{}", closed_port()), &file);
    let mut child = start(&mut command);
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{}}\n")
        .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let reply: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(reply["id"], 5);
    assert_eq!(reply["error"]["code"], -32000);
    assert_eq!(reply["error"]["data"]["reason"], "connect_failed");
    assert_eq!(output.stderr, Vec::<u8>::new());
    assert!(!stdout.contains(TOKEN));
}
