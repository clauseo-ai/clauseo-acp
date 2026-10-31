//! What goes down a window's channel: a [`Frame`] holds one message's JSON text, sent as it is.
//!
//! A message is either an event's text (`EncodedEvent::json()`, a `HostEvent` object) when it is
//! below [`FRAME_LIMIT`], or one fragment of a longer event:
//! `{"fragment":{"id":<u32>,"index":<u16>,"count":<u16>,"text":"<piece>"}}`. The pieces' texts,
//! joined in index order, are the event's text exactly; the transport joins them and parses
//! once. Every message, fragments included, is below [`FRAME_LIMIT`] bytes, so Tauri hands it
//! to the page with one `eval` instead of a second fetch round trip.

use std::fmt::Write as _;

#[cfg(test)]
use clauseo_protocol::HostEvent;
#[cfg(test)]
use serde::Serialize;
use tauri::ipc::{InvokeResponseBody, IpcResponse};

/// Every channel message is shorter than this many bytes: Tauri 2.12.4 evaluates a JSON
/// message in the page directly only below 8192 bytes (`MAX_JSON_DIRECT_EXECUTE_THRESHOLD`,
/// `tauri/src/ipc/channel.rs`) and fetches anything larger in a second request.
pub(crate) const FRAME_LIMIT: usize = 8192;

/// The bytes of a fragment message around its piece, at the widest `id`, `index` and `count`.
const FRAGMENT_OVERHEAD: usize =
    r#"{"fragment":{"id":4294967295,"index":65535,"count":65535,"text":""}}"#.len();

/// The most escaped bytes of event text one fragment carries.
const PIECE_BUDGET: usize = FRAME_LIMIT - 1 - FRAGMENT_OVERHEAD;

/// One channel message's JSON text. Tauri sends it as `InvokeResponseBody::Json`, which it
/// writes into the page as it is: the text is never parsed or encoded again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame(String);

impl Frame {
    /// The message's JSON text.
    #[cfg(test)]
    pub(crate) fn text(&self) -> &str {
        &self.0
    }

    /// Its length in bytes, what Tauri compares with its threshold.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl IpcResponse for Frame {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(InvokeResponseBody::Json(self.0))
    }
}

#[cfg(test)]
impl specta::Type for Frame {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <StreamMessage as specta::Type>::definition(types)
    }
}

/// The TypeScript shape of a channel message, for the bindings only: nothing serialises it.
#[cfg(test)]
#[derive(Serialize, specta::Type)]
#[serde(untagged)]
#[expect(
    dead_code,
    reason = "a type for the bindings only; frames are written as text"
)]
enum StreamMessage {
    /// An event, as the host encoded it.
    Event(HostEvent),
    /// A piece of an event too long for one message.
    Fragment(FragmentMessage),
}

/// `{"fragment": {...}}`.
#[cfg(test)]
#[derive(Serialize, specta::Type)]
struct FragmentMessage {
    fragment: Fragment,
}

/// One piece of an event's JSON text.
#[cfg(test)]
#[derive(Serialize, specta::Type)]
struct Fragment {
    /// Which event: per window, increasing.
    id: u32,
    /// The piece's place, from 0.
    index: u16,
    /// How many pieces the event has.
    count: u16,
    /// The piece: the next part of the event's JSON text.
    text: String,
}

/// Turns `json`, one event's text, into the frames that carry it: the text itself when it is
/// below [`FRAME_LIMIT`], else fragments with id `id`. `None` when the text needs more than
/// `u16::MAX` fragments (over 200 MB, which no event reaches).
pub(crate) fn frames(json: &str, id: u32) -> Option<Vec<Frame>> {
    if json.len() < FRAME_LIMIT {
        return Some(vec![Frame(json.to_owned())]);
    }
    let pieces = split(json);
    let count = u16::try_from(pieces.len()).ok()?;
    let mut out = Vec::with_capacity(pieces.len());
    for (index, piece) in (0..count).zip(pieces) {
        let mut text = String::with_capacity(FRAGMENT_OVERHEAD + piece.len() + piece.len() / 8);
        text.push_str(r#"{"fragment":{"id":"#);
        text.push_str(&id.to_string());
        text.push_str(r#","index":"#);
        text.push_str(&index.to_string());
        text.push_str(r#","count":"#);
        text.push_str(&count.to_string());
        text.push_str(r#","text":""#);
        escape_into(&mut text, piece);
        text.push_str(r#""}}"#);
        out.push(Frame(text));
    }
    Some(out)
}

/// Cuts `json` into pieces whose escaped length is at most [`PIECE_BUDGET`], each on a char
/// boundary.
fn split(json: &str) -> Vec<&str> {
    let mut pieces = Vec::with_capacity(json.len() / PIECE_BUDGET + 1);
    let mut start = 0;
    let mut escaped = 0;
    for (at, c) in json.char_indices() {
        let width = escaped_width(c);
        if escaped + width > PIECE_BUDGET {
            pieces.push(json.get(start..at).unwrap_or_default());
            start = at;
            escaped = 0;
        }
        escaped += width;
    }
    pieces.push(json.get(start..).unwrap_or_default());
    pieces
}

/// The bytes `c` takes inside a JSON string. The input is `serde_json` output, so it holds no
/// raw control character; any that appears is still escaped as `\u00XX`.
fn escaped_width(c: char) -> usize {
    match c {
        '"' | '\\' => 2,
        '\u{0}'..='\u{1f}' => 6,
        _ => c.len_utf8(),
    }
}

/// Appends `piece` to `out` as JSON string content. `serde_json` output holds no raw control
/// character, so the common path only finds `"` and `\\` with `memchr2` and copies the runs
/// between them; a piece with a control character takes the slower path that escapes it too.
fn escape_into(out: &mut String, piece: &str) {
    if piece.bytes().any(|b| b < 0x20) {
        escape_with_controls(out, piece);
        return;
    }
    let mut start = 0;
    for at in memchr::memchr2_iter(b'"', b'\\', piece.as_bytes()) {
        out.push_str(piece.get(start..at).unwrap_or_default());
        out.push('\\');
        // The quote or backslash itself starts the next run.
        start = at;
    }
    out.push_str(piece.get(start..).unwrap_or_default());
}

/// `escape_into` for a piece that holds a control character.
fn escape_with_controls(out: &mut String, piece: &str) {
    let mut start = 0;
    for (at, c) in piece.char_indices() {
        let escape = match c {
            '"' => "\\\"",
            '\\' => "\\\\",
            '\u{0}'..='\u{1f}' => "",
            _ => continue,
        };
        out.push_str(piece.get(start..at).unwrap_or_default());
        if escape.is_empty() {
            // Writing to a String cannot fail.
            let _infallible = write!(out, "\\u{:04x}", u32::from(c));
        } else {
            out.push_str(escape);
        }
        start = at + c.len_utf8();
    }
    out.push_str(piece.get(start..).unwrap_or_default());
}

#[cfg(test)]
mod tests;
