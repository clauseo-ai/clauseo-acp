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
//!
//! What the skipped line says ([`SkippedUpdate`]) is read from the same start, after
//! `"params"`: its `sessionId`, and in its `update` the `sessionUpdate`, `toolCallId` and
//! `status`, each a plain string, the last of a repeated key winning (a repeated `update`
//! replaces all three). Any other value is walked
//! over, counting brackets outside strings, without being decoded or checked. The read stops at
//! the update's `content`, at a key written with an escape, at anything it cannot walk, and
//! where the start ends: a field not read by then stays unknown. Both adapters write `status`
//! before `content` in a `tool_call_update`: claude-agent-acp 0.89.1 writes `_meta` (the tool's
//! name), `toolCallId`, `sessionUpdate`, `status`, then the output fields
//! (`tool-calls/renderer.js`, `result`); codex-acp 2.2.2 writes `sessionUpdate`, `toolCallId`,
//! `name`, `kind`, `title`, `status`, then `content` (`AcpToolCallRenderer.render`), but a
//! status it adds to a rendered update with no status goes after the content.
//!
//! A copied `sessionId` or `toolCallId` is at most [`MAX_ID_BYTES`]; a longer one stays unknown,
//! so what the host keeps per skipped line is small.

/// The longest `sessionId` or `toolCallId` copied from a skipped line, in bytes. Both adapters
/// write ids of a few dozen bytes.
pub(crate) const MAX_ID_BYTES: usize = 256;

/// The method a skippable line names.
pub(super) const UPDATE_METHOD: &str = "session/update";

/// The method of the notification that stands in for a skipped `session/update`, so what it
/// said reaches the inbound handler in order with the agent's other messages (as the
/// malformed-line marker does). Only the reader writes it: an agent's line naming it is
/// malformed (`envelope.rs`).
pub(crate) const SKIPPED_UPDATE_METHOD: &str = "_clauseo/skipped_update";

/// The `sessionUpdate` of a tool call's update, the only kind the host does more with.
pub(crate) const TOOL_CALL_UPDATE: &str = "tool_call_update";

/// What the start of a skipped `session/update` says (see the module doc). Each field is
/// `None` when the start did not show it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SkippedUpdate {
    /// `params.sessionId`.
    pub(crate) session: Option<String>,
    /// `params.update.sessionUpdate` is `tool_call_update` (`true`) or another kind.
    pub(crate) tool_call_update: Option<bool>,
    /// `params.update.toolCallId`.
    pub(crate) tool_call: Option<String>,
    /// `params.update.status`, when it is one of ACP's four.
    pub(crate) status: Option<&'static str>,
}

impl SkippedUpdate {
    /// The [`SKIPPED_UPDATE_METHOD`] notification for this update of `bytes` bytes. Its params
    /// hold `bytes`, then `sessionId`, `kind` (`tool_call_update` or `other`), `toolCallId` and
    /// `status`, each `null` when unknown.
    pub(crate) fn marker(&self, bytes: usize) -> String {
        let kind = self
            .tool_call_update
            .map(|tool| if tool { TOOL_CALL_UPDATE } else { "other" });
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": SKIPPED_UPDATE_METHOD,
            "params": {
                "bytes": bytes,
                "sessionId": self.session,
                "kind": kind,
                "toolCallId": self.tool_call,
                "status": self.status,
            },
        })
        .to_string()
    }
}

/// What `head`, the start of a line, says when it shows a `session/update` notification (see
/// the module doc); `None` when it does not, and the line is not skippable.
pub(crate) fn skipped_update(head: &[u8]) -> Option<SkippedUpdate> {
    let mut cursor = Cursor { head, at: 0 };
    if cursor.update_notification() != Some(true) {
        return None;
    }
    let mut skipped = SkippedUpdate::default();
    // `None` here only says where the read stopped; what it read by then stands.
    let _stopped = cursor.params(&mut skipped);
    Some(skipped)
}

/// A read position in the start of a line.
struct Cursor<'a> {
    head: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    /// `Some(true)` for a `session/update` notification, `Some(false)` for a line the start shows
    /// is something else, `None` for one it cannot read. On `Some(true)` it stands at the value
    /// of `params`.
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

    /// Reads `params`' `sessionId` and `update` into `out`. `None` where the read stops.
    fn params(&mut self, out: &mut SkippedUpdate) -> Option<()> {
        self.expect(b'{')?;
        loop {
            let key = self.plain_string()?;
            self.expect(b':')?;
            match key {
                b"sessionId" => {
                    out.session = None;
                    out.session = Some(self.text()?);
                }
                b"update" => self.update(out)?,
                _ => self.value()?,
            }
            if !self.next_member()? {
                return Some(());
            }
        }
    }

    /// Reads the update's `sessionUpdate`, `toolCallId` and `status` into `out`, up to its
    /// `content`. `None` where the read stops.
    fn update(&mut self, out: &mut SkippedUpdate) -> Option<()> {
        // A repeated `update` replaces the one before it, as the SDK reads it.
        out.tool_call_update = None;
        out.tool_call = None;
        out.status = None;
        self.expect(b'{')?;
        loop {
            let key = self.plain_string()?;
            self.expect(b':')?;
            match key {
                // What comes after the content is not read.
                b"content" => return None,
                b"sessionUpdate" => {
                    out.tool_call_update = None;
                    out.tool_call_update =
                        Some(self.plain_string()? == TOOL_CALL_UPDATE.as_bytes());
                }
                b"toolCallId" => {
                    out.tool_call = None;
                    out.tool_call = Some(self.text()?);
                }
                b"status" => {
                    out.status = None;
                    out.status = Some(status(self.plain_string()?)?);
                }
                _ => self.value()?,
            }
            if !self.next_member()? {
                return Some(());
            }
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

    /// Skips whitespace, then `,` (`Some(true)`: another member follows) or `}` (`Some(false)`:
    /// the object ends).
    fn next_member(&mut self) -> Option<bool> {
        self.whitespace();
        let next = match self.head.get(self.at)? {
            b',' => true,
            b'}' => false,
            _ => return None,
        };
        self.at += 1;
        Some(next)
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

    /// A plain string of at most [`MAX_ID_BYTES`] that is UTF-8 with no control character, as
    /// JSON allows, copied.
    fn text(&mut self) -> Option<String> {
        let bytes = self.plain_string()?;
        if bytes.len() > MAX_ID_BYTES || bytes.iter().any(|byte| *byte < 0x20) {
            return None;
        }
        std::str::from_utf8(bytes).ok().map(str::to_owned)
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

    /// Skips whitespace, then any value: a string (escapes allowed), an object or an array
    /// (walked by its brackets outside strings, its depth counted, never recursed), or a
    /// scalar. Nothing is decoded or checked.
    fn value(&mut self) -> Option<()> {
        self.whitespace();
        match self.head.get(self.at)? {
            b'"' => self.string(),
            b'{' | b'[' => self.nested(),
            _ => self.scalar(),
        }
    }

    /// Skips a string from its opening quote, escapes allowed.
    fn string(&mut self) -> Option<()> {
        self.at += 1;
        loop {
            let rest = self.head.get(self.at..)?;
            let end = memchr::memchr2(b'"', b'\\', rest)?;
            self.at += end + 1;
            if rest.get(end) == Some(&b'"') {
                return Some(());
            }
            // The escaped byte.
            self.at += 1;
        }
    }

    /// Skips an object or an array from its opening bracket to the bracket that closes it.
    fn nested(&mut self) -> Option<()> {
        let mut depth = 0_usize;
        loop {
            match self.head.get(self.at)? {
                b'"' => {
                    self.string()?;
                    continue;
                }
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        self.at += 1;
                        return Some(());
                    }
                }
                _ => {}
            }
            self.at += 1;
        }
    }
}

/// One of ACP's four tool call statuses, as written.
fn status(text: &[u8]) -> Option<&'static str> {
    Some(match text {
        b"pending" => "pending",
        b"in_progress" => "in_progress",
        b"completed" => "completed",
        b"failed" => "failed",
        _ => return None,
    })
}
