//! The tail's whole-line opening when redaction shrinks the end of stderr to exactly the tail.

use super::*;

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
    let tail = tail(&ring_with(stderr.as_bytes()), &no_rules());
    assert_eq!(tail, format!("[redacted]\n{last_lines}"));
}
