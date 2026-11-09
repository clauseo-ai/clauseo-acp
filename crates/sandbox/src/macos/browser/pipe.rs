//! Chrome's `DevTools` pipe (`--remote-debugging-pipe`): Chrome reads commands from its fd 3 and
//! writes replies and events to its fd 4, each message one JSON text ended by a NUL byte. No
//! socket: the browser profile allows no inet listener (0038), so this pipe is how `DevTools`
//! leaves Chrome. [`BrowserLauncher::launch_chrome`](super::BrowserLauncher::launch_chrome)
//! hands this process's ends over as a [`CdpPipe`]; the framing here works on any reader and
//! writer, so it is tested on every target.

use std::io::{self, BufRead, BufReader, PipeReader, PipeWriter, Read, Write};

/// The flag Chrome is started with. The launcher adds it and drops any
/// `--remote-debugging-port` or `--remote-debugging-address`.
pub const PIPE_FLAG: &str = "--remote-debugging-pipe";

/// Chrome's descriptor for commands in: it reads, this process writes.
pub const CHROME_IN_FD: i32 = 3;

/// Chrome's descriptor for replies and events out: it writes, this process reads.
pub const CHROME_OUT_FD: i32 = 4;

/// The longest message [`MessageReader`] takes, NUL excluded: 128 MiB. A full-page screenshot
/// in base64 stays far under it; a Chrome that sends more without a NUL is cut off, so it
/// cannot make this process (outside every jail) hold without bound.
pub const MAX_MESSAGE: usize = 128 * 1024 * 1024;

/// This process's ends of one Chrome's pipe. Dropping `to_chrome` closes Chrome's fd 3, which
/// Chrome takes as a request to exit.
#[derive(Debug)]
pub struct CdpPipe {
    /// Write commands here ([`write_message`]); Chrome reads them on its fd 3.
    pub to_chrome: PipeWriter,
    /// Read replies and events here ([`MessageReader`]); Chrome writes them on its fd 4.
    pub from_chrome: PipeReader,
}

impl CdpPipe {
    /// Sends one message.
    pub fn send(&mut self, message: &[u8]) -> io::Result<()> {
        write_message(&mut self.to_chrome, message)
    }

    /// The pipe split into a writer and a reader of whole messages.
    pub fn split(self) -> (PipeWriter, MessageReader<PipeReader>) {
        (self.to_chrome, MessageReader::new(self.from_chrome))
    }
}

/// Writes `message` and its NUL, then flushes. Refuses a message that holds a NUL itself
/// ([`io::ErrorKind::InvalidInput`]), since Chrome would read it as two; JSON never needs one
/// (it is escaped as `\u0000`).
pub fn write_message(to: &mut impl Write, message: &[u8]) -> io::Result<()> {
    if message.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a DevTools message may not hold a NUL byte",
        ));
    }
    to.write_all(message)?;
    to.write_all(&[0])?;
    to.flush()
}

/// Reads whole messages, each ended by a NUL.
#[derive(Debug)]
pub struct MessageReader<R> {
    from: BufReader<R>,
    max: usize,
    spent: bool,
}

impl<R: Read> MessageReader<R> {
    /// A reader of messages up to [`MAX_MESSAGE`] long.
    pub fn new(from: R) -> MessageReader<R> {
        MessageReader::with_max(from, MAX_MESSAGE)
    }

    /// A reader of messages up to `max` bytes long.
    pub fn with_max(from: R, max: usize) -> MessageReader<R> {
        MessageReader {
            from: BufReader::new(from),
            max,
            spent: false,
        }
    }

    /// The next message without its NUL; `None` at the end of the stream between messages.
    /// A stream that ends inside a message is [`io::ErrorKind::UnexpectedEof`]; a message
    /// longer than the limit is [`io::ErrorKind::InvalidData`], and the reader is then spent.
    pub fn next_message(&mut self) -> io::Result<Option<Vec<u8>>> {
        if self.spent {
            return Err(over_limit());
        }
        let mut message = Vec::new();
        loop {
            let buf = self.from.fill_buf()?;
            if buf.is_empty() {
                return if message.is_empty() {
                    Ok(None)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the DevTools pipe ended inside a message",
                    ))
                };
            }
            let (take, done) = match buf.iter().position(|b| *b == 0) {
                Some(at) => (at, true),
                None => (buf.len(), false),
            };
            if message.len() + take > self.max {
                self.spent = true;
                return Err(over_limit());
            }
            message.extend_from_slice(buf.get(..take).unwrap_or_default());
            self.from.consume(take + usize::from(done));
            if done {
                return Ok(Some(message));
            }
        }
    }
}

fn over_limit() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "a DevTools message is over the limit",
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn a_message_is_its_bytes_and_a_nul() {
        let mut out = Vec::new();
        write_message(&mut out, br#"{"id":1,"method":"Browser.getVersion"}"#).unwrap();
        write_message(&mut out, b"{}").unwrap();
        assert_eq!(out, b"{\"id\":1,\"method\":\"Browser.getVersion\"}\0{}\0");
    }

    #[test]
    fn a_nul_inside_a_message_is_refused_and_nothing_is_written() {
        let mut out = Vec::new();
        let e = write_message(&mut out, b"{\"a\":\"\0\"}").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(out, Vec::<u8>::new());
    }

    #[test]
    fn messages_are_split_at_each_nul_and_the_end_is_none() {
        let mut r = MessageReader::new(Cursor::new(b"{\"id\":1}\0{}\0\0".to_vec()));
        assert_eq!(r.next_message().unwrap().unwrap(), b"{\"id\":1}");
        assert_eq!(r.next_message().unwrap().unwrap(), b"{}");
        assert_eq!(r.next_message().unwrap().unwrap(), b"");
        assert!(r.next_message().unwrap().is_none());
        assert!(r.next_message().unwrap().is_none());
    }

    /// A reader that hands out one byte per read, so every message spans many buffers.
    struct Trickle(Cursor<Vec<u8>>);

    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(1);
            self.0.read(&mut buf[..n])
        }
    }

    #[test]
    fn a_message_split_across_reads_is_whole() {
        let mut r = MessageReader::new(Trickle(Cursor::new(b"{\"id\":7}\0[]\0".to_vec())));
        assert_eq!(r.next_message().unwrap().unwrap(), b"{\"id\":7}");
        assert_eq!(r.next_message().unwrap().unwrap(), b"[]");
        assert!(r.next_message().unwrap().is_none());
    }

    #[test]
    fn a_stream_that_ends_inside_a_message_is_an_error() {
        let mut r = MessageReader::new(Cursor::new(b"{}\0{\"id\"".to_vec()));
        assert_eq!(r.next_message().unwrap().unwrap(), b"{}");
        let e = r.next_message().unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn a_message_over_the_limit_is_refused_and_the_reader_is_spent() {
        let mut r = MessageReader::with_max(Cursor::new(b"1234\x0012345\x00{}\x00".to_vec()), 4);
        assert_eq!(r.next_message().unwrap().unwrap(), b"1234");
        let e = r.next_message().unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        let e = r.next_message().unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        let mut r = MessageReader::with_max(Trickle(Cursor::new(b"123456789\0".to_vec())), 4);
        assert_eq!(
            r.next_message().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn a_pipe_carries_messages_both_ways() {
        let (to_chrome_r, to_chrome) = io::pipe().unwrap();
        let (from_chrome, from_chrome_w) = io::pipe().unwrap();
        let mut pipe = CdpPipe {
            to_chrome,
            from_chrome,
        };
        pipe.send(br#"{"id":1,"method":"Browser.getVersion"}"#)
            .unwrap();
        // The stand-in Chrome: reads the command, answers it, closes its end.
        let mut chrome_in = MessageReader::new(to_chrome_r);
        let command = chrome_in.next_message().unwrap().unwrap();
        assert_eq!(command, br#"{"id":1,"method":"Browser.getVersion"}"#);
        let mut chrome_out = from_chrome_w;
        write_message(&mut chrome_out, br#"{"id":1,"result":{}}"#).unwrap();
        drop(chrome_out);
        let (to_chrome, mut replies) = pipe.split();
        assert_eq!(
            replies.next_message().unwrap().unwrap(),
            br#"{"id":1,"result":{}}"#
        );
        assert!(replies.next_message().unwrap().is_none());
        // Closing this end is Chrome's end of input.
        drop(to_chrome);
        assert!(chrome_in.next_message().unwrap().is_none());
    }
}
