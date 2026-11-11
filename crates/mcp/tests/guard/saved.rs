//! A PDF link Chrome saves rather than shows, at the guard's level with the stand-ins: the file
//! lands in the matter's downloads folder (`--downloads`), and the `open` that started it is
//! answered as a success naming it (`downloads/saved.rs`).

use std::ffi::OsString;
use std::process::Command;

use serde_json::{Value, json};

use crate::support::{GUARD, Guard, Scratch, example, guard_args, host_env};
use crate::write;

/// The texts of `answer`'s content blocks.
fn texts(answer: &Value) -> Vec<String> {
    answer["result"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str().map(str::to_owned))
        .collect()
}

/// A guard over the `daemon_stub` with `--downloads <scratch>/matter/downloads`, which it makes.
fn guard_for_matter(scratch: &Scratch) -> (Guard, std::path::PathBuf) {
    let matter = scratch.path().join("matter");
    write::create_dir_all(&matter).unwrap();
    let downloads = matter.join("downloads");
    let mut args = guard_args(scratch, &example("daemon_stub"), &[]);
    // After `browser-guard --agent-browser <stub> --cwd <cwd>`.
    let tail = args.split_off(5);
    args.extend([OsString::from("--downloads"), downloads.clone().into()]);
    args.extend(tail);
    let mut command = Command::new(GUARD);
    command.args(args).envs(host_env(scratch));
    (Guard::start(command), downloads)
}

#[test]
fn opening_a_pdf_link_saves_it_in_the_matter_and_says_so() {
    let scratch = Scratch::new("pdf_saved");
    let (mut guard, downloads) = guard_for_matter(&scratch);
    let url = "https://www.sci.gov.in/judgment.pdf";
    guard.send(
        &json!({"jsonrpc": "2.0", "id": "pdf", "method": "tools/call",
        "params": {"name": "agent_browser_open", "arguments": {"url": url}}}),
    );
    let answer = guard.read();
    assert!(downloads.is_dir(), "no matter downloads folder: {answer}");
    assert_eq!(answer["id"], "pdf", "{answer}");
    assert!(answer.get("error").is_none(), "{answer}");
    assert_ne!(answer["result"]["isError"], true, "a success: {answer}");
    assert_eq!(
        texts(&answer),
        ["Saved judgment.pdf to downloads/ in the matter."],
        "{answer}"
    );
    assert_eq!(
        std::fs::read(downloads.join("judgment.pdf")).unwrap(),
        b"%PDF-1.4 stub judgment"
    );
    // Told once: the next result carries no note.
    let next = guard.call("after");
    assert!(texts(&next).is_empty(), "told twice: {next}");
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
}
