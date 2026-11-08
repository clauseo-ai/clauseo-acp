//! The fake denial pipe: the outer it can open, its client, and what the client writes.

use std::collections::VecDeque;
use std::time::Duration;

use super::super::feed::Got;
use super::super::{DenialPipe, Peer};

/// The fake's denial pipe name.
pub(super) const PIPE: &str = r"\\.\pipe\clauseo-run-0123456789abcdef0123456789abcdef-denials";

/// A denial pipe: the outer it can open, the client that connects (if any), and what the
/// client writes, chunk by chunk; then the end, or a failed read. With `exits_after`, the
/// outer exits once that many reads have got bytes: the chunks left came after its exit.
#[derive(Default)]
pub(super) struct FakePipe {
    pub(super) outer: Option<Peer>,
    pub(super) client: Option<Peer>,
    pub(super) chunks: VecDeque<Vec<u8>>,
    pub(super) read_fails: bool,
    pub(super) exits_after: Option<usize>,
}

impl DenialPipe for FakePipe {
    fn watch(&mut self, pid: u32) -> std::io::Result<Peer> {
        self.outer
            .filter(|outer| outer.pid == pid)
            .ok_or_else(|| std::io::Error::other("no such process"))
    }

    fn accept(&mut self, _within: Duration) -> std::io::Result<Option<Peer>> {
        Ok(self.client)
    }

    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<Got> {
        if let Some(reads) = &mut self.exits_after {
            if *reads == 0 {
                return Ok(Got::Exited);
            }
            *reads -= 1;
        }
        let Some(mut chunk) = self.chunks.pop_front() else {
            return if self.read_fails {
                Err(std::io::Error::other("the pipe broke"))
            } else {
                Ok(Got::Closed)
            };
        };
        if chunk.len() > buf.len() {
            self.chunks.push_front(chunk.split_off(buf.len()));
        }
        buf[..chunk.len()].copy_from_slice(&chunk);
        Ok(Got::Bytes(chunk.len()))
    }
}
