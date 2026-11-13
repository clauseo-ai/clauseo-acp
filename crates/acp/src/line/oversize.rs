//! Which line over [`MAX_LINE_BYTES`](super::MAX_LINE_BYTES) the reader may skip instead of
//! closing the connection: a `session/update` notification, read from the start of the line the
//! reader holds. Skipping one loses that update and nothing else. Skipping a response would
//! leave a request waiting forever, and skipping a request would leave the agent waiting, so
//! anything the start does not show to be a `session/update` notification stays fatal.
//!
//! The start shows it when the line opens an object whose keys before `"params"` include
//! `"method"` with the plain string `"session/update"` (the last such key, as the SDK reads a
//! repeated key) and no `"id"`. Only plain strings, numbers, `true`, `false` and `null` may come
//! before `"params"`; a key or a `method` value written with an escape, an object or an array
//! there, or a start that ends before `"params"`, refuses the skip. JSON whitespace between the
//! tokens is allowed. Both adapters write `{"jsonrpc":"2.0","method":"session/update","params":`
//! (the recorded replays in `tests/fixtures`).
//!
//! An `"id"` after `"params"` is not seen: such a line is a request the agent then never hears
//! back on, which harms only that agent.

/// The method a skippable line names.
pub(super) const UPDATE_METHOD: &str = "session/update";

/// Whether `head`, the start of a line, shows a `session/update` notification (see the module
/// doc).
pub(super) fn is_update_notification(head: &[u8]) -> bool {
    let mut cursor = Cursor { head, at: 0 };
    cursor.update_notification().unwrap_or(false)
}

/// A read position in the start of a line.
struct Cursor<'a> {
    head: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    /// `Some(true)` for a `session/update` notification, `Some(false)` for a line the start shows
    /// is something else, `None` for one it cannot read.
    fn update_notification(&mut self) -> Option<bool> {
        self.expect(b'{')?;
        let mut method: Option<&[u8]> = None;
        loop {
            let key = self.plain_string()?;
            self.expect(b':')?;
            match key {
                b"params" => return Some(method == Some(UPDATE_METHOD.as_bytes())),
                b"id" => return Some(false),
                b"method" => method = Some(self.plain_string()?),
                _ => self.scalar()?,
            }
            self.expect(b',')?;
        }
    }

    /// Skips JSON whitespace.
    fn whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.head.get(self.at) {
            self.at += 1;
        }
    }

    /// Skips whitespace, then `byte`.
    fn expect(&mut self, byte: u8) -> Option<()> {
        self.whitespace();
        (self.head.get(self.at) == Some(&byte)).then(|| self.at += 1)
    }

    /// Skips whitespace, then a string with no escape, giving its bytes.
    fn plain_string(&mut self) -> Option<&'a [u8]> {
        self.expect(b'"')?;
        let rest = self.head.get(self.at..)?;
        let end = memchr::memchr2(b'"', b'\\', rest)?;
        if rest.get(end) != Some(&b'"') {
            return None;
        }
        self.at += end + 1;
        rest.get(..end)
    }

    /// Skips whitespace, then a plain string, a number, `true`, `false` or `null`.
    fn scalar(&mut self) -> Option<()> {
        self.whitespace();
        if self.head.get(self.at) == Some(&b'"') {
            return self.plain_string().map(|_| ());
        }
        let start = self.at;
        while let Some(byte) = self.head.get(self.at) {
            if !matches!(byte, b'0'..=b'9' | b'a'..=b'z' | b'+' | b'-' | b'.' | b'E') {
                break;
            }
            self.at += 1;
        }
        (self.at > start).then_some(())
    }
}
