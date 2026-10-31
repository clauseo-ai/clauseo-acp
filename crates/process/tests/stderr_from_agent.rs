//! What reaches a stderr reader and the exit report from an agent's stderr, with the agent
//! writing exactly the bytes a test chose: it is this test binary run again
//! (`support/self_agent.rs`), so no scenario file has to be written.

#[cfg(test)]
#[path = "support/self_agent.rs"]
mod self_agent;

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;

use async_io::block_on;
use clauseo_process::{ExitReport, Launched, SpawnOptions};
use futures::join;

/// The agent the tests here start, not a check: run as one, it writes its input to stderr as
/// it is and returns. In a normal run it has no input and returns at once.
#[test]
fn agent_writes_its_input_to_stderr() {
    if let Some(input) = self_agent::role() {
        let mut stderr = std::io::stderr().lock();
        stderr.write_all(input.as_bytes()).expect("write stderr");
        stderr.flush().expect("flush stderr");
    }
}

/// Runs the agent with `stderr` as its input and `opts`, reads its stderr to the end by lines,
/// and returns its exit report and the lines.
#[cfg(test)]
fn run(stderr: &str, opts: &SpawnOptions) -> (ExitReport, Vec<String>) {
    let Launched {
        stdout: _stdout,
        stderr: pump,
        mut handle,
        ..
    } = self_agent::launch("agent_writes_its_input_to_stderr", stderr, opts).unwrap();
    let mut lines = Vec::new();
    let (report, pumped) = block_on(async {
        join!(
            handle.exited(),
            pump.lines(|line| lines.push(line.to_owned()))
        )
    });
    pumped.unwrap();
    assert_eq!(
        report.code(),
        Some(0),
        "the agent failed; its stderr: {lines:?}"
    );
    (report, lines)
}

/// The home folder given in `SpawnOptions::home` shows as `~` in the stderr tail.
#[test]
fn home_folder_shows_as_tilde_in_the_tail() {
    let home = "/home/clauseo-test-user";
    let opts = SpawnOptions::new().home(PathBuf::from(home));
    let (report, _) = run(&format!("cannot read {home}/project/notes.md\n"), &opts);
    assert_eq!(report.stderr_tail(), "cannot read ~/project/notes.md\n");
}

/// A stderr line of exactly 16 KiB reaches `StderrPump::lines` whole, and a longer one comes in
/// 16 KiB pieces, the rest last.
#[test]
fn lines_pass_16_kib_whole_and_cut_longer_ones_into_16_kib_pieces() {
    let whole = "w".repeat(16 * 1024);
    let long = "l".repeat(20_000);
    let (_, lines) = run(&format!("{whole}\n{long}\n"), &SpawnOptions::new());
    let lengths: Vec<usize> = lines.iter().map(String::len).collect();
    assert_eq!(lengths, [16_384, 16_384, 20_000 - 16_384]);
    assert_eq!(lines.concat(), whole + &long);
}

/// The tail opens on a whole line even when the end of stderr, once redacted, is exactly the
/// tail's 4096 bytes long and starts partway through a line: the part line is left out.
#[test]
fn tail_opens_on_a_whole_line_when_redaction_shrinks_the_end_to_the_tail_size() {
    // The tail redacts the last 16 KiB of stderr first. Laid out so those 16 KiB start 100
    // bytes before the end of a line of `a`s, then hold a 12,299-byte `sk-` key (which becomes
    // `[redacted]`) and 3,984 bytes of short lines: 100 + 1 + 11 + 3,984 = 4,096 once redacted.
    let mut last_lines = String::new();
    for n in 0..83 {
        writeln!(
            last_lines,
            "line {n:02} of the agent's stderr, 48 bytes long..."
        )
        .unwrap();
    }
    assert_eq!(last_lines.len(), 3_984);
    let key = format!("sk-{}\n", "k".repeat(12_295));
    let stderr = format!("{}\n{key}{last_lines}", "a".repeat(1_100));
    assert_eq!(stderr.len() - 1_000, 16 * 1024);
    let (report, _) = run(&stderr, &SpawnOptions::new());
    assert_eq!(report.stderr_tail(), format!("[redacted]\n{last_lines}"));
}
