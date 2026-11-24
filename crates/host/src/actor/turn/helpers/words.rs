//! A helper's words held for the next commit: one run per helper, in first-word order, and
//! the cut that keeps each logged `HelperText` within the inline limit.

use clauseo_protocol::{HelperId, INLINE_LIMIT_BYTES, TurnId, escaped_len};

/// Words held past which the commit goes at once: two events' worth.
const FULL_BYTES: usize = 2 * INLINE_LIMIT_BYTES;

/// One helper's words not logged yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Run {
    pub(super) helper: HelperId,
    pub(super) turn: TurnId,
    pub(super) text: String,
}

/// The runs held, in first-word order.
#[derive(Debug, Default)]
pub(super) struct Held {
    runs: Vec<Run>,
    bytes: usize,
}

/// What a push did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Pushed {
    /// The held words were empty before it: the caller arms the durable batch's timer.
    pub(super) first: bool,
    /// The held words passed [`FULL_BYTES`]: the caller commits them now.
    pub(super) full: bool,
}

impl Held {
    /// Adds `text` to the run of `helper` in `turn`, or opens one.
    pub(super) fn push(&mut self, helper: HelperId, turn: TurnId, text: &str) -> Pushed {
        let first = self.runs.is_empty();
        match self
            .runs
            .iter_mut()
            .find(|run| run.helper == helper && run.turn == turn)
        {
            Some(run) => run.text.push_str(text),
            None => self.runs.push(Run {
                helper,
                turn,
                text: text.to_owned(),
            }),
        }
        self.bytes += text.len();
        Pushed {
            first,
            full: self.bytes > FULL_BYTES,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Every run held, emptied out; runs with no text are left out.
    pub(super) fn take(&mut self) -> Vec<Run> {
        self.bytes = 0;
        let mut runs = std::mem::take(&mut self.runs);
        runs.retain(|run| !run.text.is_empty());
        runs
    }
}

/// `text` in pieces of at most `limit` bytes after JSON escaping, cut between characters;
/// `text` itself when it fits.
pub(super) fn cut(text: String, limit: usize) -> Vec<String> {
    // Escaping never shrinks text, so a text within the limit after escaping needs no cut.
    if text.len() <= limit && escaped_len(&text) <= limit {
        return vec![text];
    }
    let mut pieces = Vec::new();
    let mut piece = String::new();
    let mut cost = 0;
    let mut buf = [0_u8; 4];
    for ch in text.chars() {
        let one = escaped_len(ch.encode_utf8(&mut buf));
        if cost + one > limit && !piece.is_empty() {
            pieces.push(std::mem::take(&mut piece));
            cost = 0;
        }
        piece.push(ch);
        cost += one;
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use clauseo_protocol::escaped_len;

    use super::{FULL_BYTES, Held, cut};
    use clauseo_protocol::{HelperId, TurnId};

    use crate::pipeline::tests::support::id;

    fn helper(n: u64) -> HelperId {
        id(n)
    }

    fn turn(n: u64) -> TurnId {
        id(100 + n)
    }

    #[test]
    fn a_text_within_the_limit_is_one_piece() {
        assert_eq!(cut("short".to_owned(), 10), vec!["short".to_owned()]);
    }

    #[test]
    fn a_long_text_is_cut_between_characters_within_the_escaped_limit() {
        let text = "a\"é\n".repeat(30);
        let pieces = cut(text.clone(), 10);
        assert!(pieces.len() > 1);
        assert!(
            pieces.iter().all(|piece| escaped_len(piece) <= 10),
            "{pieces:?}"
        );
        assert_eq!(pieces.concat(), text);
    }

    #[test]
    fn words_join_their_helpers_run_in_first_word_order() {
        let mut held = Held::default();
        assert!(held.push(helper(1), turn(1), "One ").first);
        assert!(!held.push(helper(2), turn(1), "Two").first);
        held.push(helper(1), turn(1), "more.");
        let runs = held.take();
        let texts: Vec<_> = runs
            .iter()
            .map(|run| (run.helper, run.text.as_str()))
            .collect();
        assert_eq!(texts, [(helper(1), "One more."), (helper(2), "Two")]);
        assert!(held.is_empty());
    }

    #[test]
    fn words_past_two_events_worth_ask_for_a_commit() {
        let mut held = Held::default();
        let long = "x".repeat(FULL_BYTES);
        assert!(!held.push(helper(1), turn(1), &long).full);
        assert!(held.push(helper(1), turn(1), "y").full);
    }
}
