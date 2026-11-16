//! The delivered-file notes and the answer to an `open` that turned into a download, with a real
//! mover on scratch folders and Chrome's part played by the test.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::super::{Held, Mover, marks};
use super::{IN_PROGRESS, Saved, note, open_reply};
use crate::browser_guard::{now, pause};
use crate::write;

/// A fresh `<temp>/clauseo-mcp-saved-<test>-<pid>` with an empty `staging` and a matter whose
/// `downloads` folder is empty.
fn scratch(test: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-saved-{test}-{}", std::process::id()));
    drop(write::remove_dir_all(&dir));
    let (staging, downloads) = (dir.join("staging"), dir.join("matter").join("downloads"));
    write::create_dir_all(&staging).unwrap();
    write::create_dir_all(&downloads).unwrap();
    (dir, staging, downloads)
}

/// agent-browser's reply to an `open` whose navigation Chrome turned into a download.
fn aborted(id: &str) -> Vec<u8> {
    json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text",
        "text": "Navigation failed: net::ERR_ABORTED at https://www.sci.gov.in/j.pdf"}],
        "isError": true}})
    .to_string()
    .into_bytes()
}

fn parse(line: &[u8]) -> Value {
    serde_json::from_slice(line).unwrap()
}

fn texts(reply: &Value) -> Vec<&str> {
    reply["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["text"].as_str().unwrap())
        .collect()
}

#[test]
fn a_delivered_file_is_told_in_the_next_result_once() {
    let saved = Saved::default();
    let reply = br#"{"jsonrpc":"2.0","id":7,"result":{"content":[{"type":"text","text":"ok"}]}}"#;
    assert_eq!(saved.note_into(reply), None, "nothing to tell");
    saved.record("CLAUDE (download).md");
    let told = parse(&saved.note_into(reply).unwrap());
    assert_eq!(
        texts(&told),
        [
            "ok",
            "Saved CLAUDE (download).md to downloads/ in the matter."
        ]
    );
    assert_eq!(saved.note_into(reply), None, "told once");
    // An error reply carries no note; it waits for a result.
    saved.record("a.pdf");
    let error = br#"{"jsonrpc":"2.0","id":8,"error":{"code":-32603,"message":"x"}}"#;
    assert_eq!(saved.note_into(error), None);
    assert_eq!(saved.take(), [note("a.pdf")]);
}

/// The required path: an `open` of a PDF URL fails with `net::ERR_ABORTED` while Chrome writes
/// the file; the mover delivers it into the matter, under its protected-name rule, and the
/// `open` is answered as a success that names it, no error.
#[test]
fn an_aborted_open_waits_for_the_mover_and_says_the_file_was_saved() {
    let (dir, staging, downloads) = scratch("open-saved");
    let saved = Arc::new(Saved::default());
    let mover = Mover::start(
        staging.clone(),
        downloads.clone(),
        Arc::new(Held::default()),
        Arc::clone(&saved),
    );
    // Chrome: the page's name is a protected one; the file is marked, then renamed when done.
    let partial = staging.join("AGENTS.md.crdownload");
    write::write(&partial, "judgment").unwrap();
    marks::ensure(&partial).unwrap();
    let chrome = std::thread::spawn({
        let staging = staging.clone();
        move || {
            pause(Duration::from_millis(400));
            write::replace(&partial, &staging.join("AGENTS.md")).unwrap();
        }
    });
    let answer = open_reply(&aborted("o1"), &saved, &staging, Duration::from_secs(10)).unwrap();
    chrome.join().unwrap();
    let answer = parse(&answer);
    assert_eq!(answer["id"], "o1");
    assert!(answer["result"].get("isError").is_none(), "{answer}");
    assert_eq!(
        texts(&answer),
        ["Saved AGENTS (download).md to downloads/ in the matter."]
    );
    assert_eq!(
        std::fs::read(downloads.join("AGENTS (download).md")).unwrap(),
        b"judgment"
    );
    assert!(!downloads.join("AGENTS.md").exists());
    assert!(marks::marked(&downloads.join("AGENTS (download).md")));
    assert!(
        saved.take().is_empty(),
        "told in the open's answer, not again"
    );
    mover.finish();
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_download_still_written_at_the_bound_is_in_progress_not_an_error() {
    let (dir, staging, _) = scratch("open-progress");
    write::write(&staging.join("big.pdf.crdownload"), "half").unwrap();
    let started = now();
    let answer = open_reply(
        &aborted("o2"),
        &Saved::default(),
        &staging,
        Duration::from_millis(300),
    );
    assert!(started.elapsed() >= Duration::from_millis(300));
    let answer = parse(&answer.unwrap());
    assert!(answer["result"].get("isError").is_none(), "{answer}");
    assert_eq!(texts(&answer), [IN_PROGRESS]);
    write::remove_dir_all(&dir).unwrap();
}

/// No download in sight by the bound: agent-browser's error goes on. A reply that is no
/// `ERR_ABORTED` failure is left at once, waiting for nothing.
#[test]
fn other_replies_go_on_as_read() {
    let (dir, staging, _) = scratch("open-other");
    let saved = Saved::default();
    assert_eq!(
        open_reply(&aborted("o3"), &saved, &staging, Duration::from_millis(200)),
        None
    );
    saved.record("early.pdf");
    let started = now();
    for line in [
        br#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"ok"}]}}"#.to_vec(),
        br#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"net::ERR_NAME_NOT_RESOLVED"}],"isError":true}}"#.to_vec(),
    ] {
        assert_eq!(open_reply(&line, &saved, &staging, Duration::from_secs(10)), None);
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(
        saved.take(),
        [note("early.pdf")],
        "left for the next result"
    );
    write::remove_dir_all(&dir).unwrap();
}
