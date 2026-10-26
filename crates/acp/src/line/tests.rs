use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::io::AsyncRead;

use super::*;

/// Hands out its bytes `step` at a time, as a pipe that delivers a little per read.
struct Trickle {
    bytes: Vec<u8>,
    at: usize,
    step: usize,
}

impl AsyncRead for Trickle {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let end = (this.at + this.step).min(this.bytes.len());
        let count = (end - this.at).min(buf.len());
        buf[..count].copy_from_slice(&this.bytes[this.at..this.at + count]);
        this.at += count;
        Poll::Ready(Ok(count))
    }
}

fn lines(bytes: Vec<u8>, step: usize) -> Result<Vec<Vec<u8>>, SplitError> {
    let mut splitter = LineSplitter::new(Trickle { bytes, at: 0, step });
    futures::executor::block_on(async {
        let mut lines = Vec::new();
        while let Some(line) = splitter.next_line().await? {
            lines.push(line);
        }
        Ok(lines)
    })
}

#[test]
fn line_split_across_reads_kept_whole() {
    // A 200 KiB line spans several fills of the 64 KiB buffer; small reads split it further.
    let long: Vec<u8> = (0..200 * 1024)
        .map(|i| b'a' + u8::try_from(i % 26).unwrap())
        .collect();
    let mut input = b"{\"a\":1}\n".to_vec();
    input.extend_from_slice(&long);
    input.push(b'\n');
    input.extend_from_slice(b"last without newline");
    for step in [1, 7, 4096, 64 * 1024, usize::MAX / 2] {
        let lines = lines(input.clone(), step).unwrap();
        assert_eq!(lines.len(), 3, "step {step}");
        assert_eq!(lines[0], b"{\"a\":1}");
        assert_eq!(lines[1], long, "step {step}");
        assert_eq!(lines[2], b"last without newline");
    }
}

#[test]
fn line_at_8_mib_passes_and_one_byte_more_fails() {
    let mut exact = vec![b'x'; MAX_LINE_BYTES];
    exact.push(b'\n');
    assert_eq!(lines(exact, 1024 * 1024).unwrap()[0].len(), MAX_LINE_BYTES);
    // Over the cap within one buffer, and over it across reads.
    for step in [usize::MAX / 2, 1024 * 1024] {
        let mut over = vec![b'x'; MAX_LINE_BYTES + 1];
        over.push(b'\n');
        assert!(
            matches!(lines(over, step), Err(SplitError::TooLong)),
            "step {step}"
        );
    }
    // Over the cap with no newline at all.
    let endless = vec![b'x'; MAX_LINE_BYTES + 1];
    assert!(matches!(
        lines(endless, 1024 * 1024),
        Err(SplitError::TooLong)
    ));
}

#[test]
fn empty_input_has_no_lines() {
    assert_eq!(lines(Vec::new(), 1).unwrap(), Vec::<Vec<u8>>::new());
    assert_eq!(lines(b"\n\n".to_vec(), 1).unwrap(), [b"", b""]);
}
