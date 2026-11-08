//! One thread's denials in one turn: its counts toward the pause, its cards, the refused targets
//! it has seen, and the tool calls it watches.

use std::collections::BTreeMap;

use clauseo_protocol::{DenialId, DenialKind, MatterId, ToolCallId, TurnId};

use super::{MAX_CARDS_PER_TURN, PAUSE_IN_A_ROW, PAUSE_IN_TURN, Record, Recorded};
use crate::actor::turn::denials::offer::identity::Target;
use crate::error::Error;

/// The most refused targets one turn remembers for its one count per refusal; past them each
/// report counts on its own.
const MAX_TARGETS_PER_TURN: usize = 128;

/// One thread's denials, all in `turn`.
#[derive(Debug)]
pub(super) struct TurnDenials {
    pub(super) turn: Option<TurnId>,
    pub(super) in_a_row: u32,
    pub(super) in_turn: u32,
    /// A counted denial came since the last tool call ended.
    pub(super) since_tool_end: bool,
    paused: bool,
    cards: Vec<Card>,
    targets: Vec<Seen>,
    /// The tool calls running, with the paths each named.
    pub(super) calls: BTreeMap<ToolCallId, Vec<String>>,
    capped_logged: bool,
}

impl Default for TurnDenials {
    fn default() -> Self {
        Self::new(None)
    }
}

/// One card.
#[derive(Debug)]
struct Card {
    kind: DenialKind,
    target: Target,
    offered: bool,
    denial: DenialId,
    count: u32,
}

/// One refused target, and who reported it.
#[derive(Debug)]
struct Seen {
    target: Target,
    /// An OS report of it came.
    os: bool,
    /// Tool-call reports of it counted toward the pause that no OS report has matched yet.
    tool_counted: u32,
}

impl TurnDenials {
    pub(super) fn new(turn: Option<TurnId>) -> Self {
        Self {
            turn,
            in_a_row: 0,
            in_turn: 0,
            since_tool_end: false,
            paused: false,
            cards: Vec::new(),
            targets: Vec::new(),
            calls: BTreeMap::new(),
            capped_logged: false,
        }
    }

    /// Counts `n` toward the pause; the pause, when it is due now and has not come yet.
    pub(super) fn count(&mut self, n: u32) -> Option<(TurnId, u16)> {
        if n > 0 {
            self.in_a_row = self.in_a_row.saturating_add(n);
            self.in_turn = self.in_turn.saturating_add(n);
            self.since_tool_end = true;
        }
        let due = self.in_a_row >= PAUSE_IN_A_ROW || self.in_turn >= PAUSE_IN_TURN;
        match self.turn {
            Some(turn) if due && !self.paused => {
                self.paused = true;
                Some((turn, u16::try_from(self.in_turn).unwrap_or(u16::MAX)))
            }
            _ => None,
        }
    }

    /// One refusal, counted once however many sources report it: an OS report counted at the
    /// intake takes back a tool call's count of the same target; a tool call's report counts
    /// here unless an OS report of the target came first.
    pub(super) fn count_once(&mut self, record: &Record<'_>) -> Option<(TurnId, u16)> {
        let asked = record.asked?;
        let at = self.targets.iter().position(|seen| seen.target.same(asked));
        if record.os {
            match at.and_then(|at| self.targets.get_mut(at)) {
                Some(seen) => {
                    seen.os = true;
                    if seen.tool_counted > 0 {
                        seen.tool_counted = seen.tool_counted.saturating_sub(1);
                        self.in_a_row = self.in_a_row.saturating_sub(1);
                        self.in_turn = self.in_turn.saturating_sub(1);
                    }
                }
                None => self.remember(asked, true, 0),
            }
            return None;
        }
        match at.and_then(|at| self.targets.get_mut(at)) {
            Some(seen) if seen.os => return None,
            Some(seen) => seen.tool_counted = seen.tool_counted.saturating_add(1),
            None => self.remember(asked, false, 1),
        }
        self.count(1)
    }

    fn remember(&mut self, target: &Target, os: bool, tool_counted: u32) {
        if self.targets.len() < MAX_TARGETS_PER_TURN {
            self.targets.push(Seen {
                target: target.clone(),
                os,
                tool_counted,
            });
        }
    }

    /// Whether an OS report of the same file as `asked`, by identity, came.
    pub(super) fn confirmed(&self, asked: &Target) -> bool {
        self.targets
            .iter()
            .any(|seen| seen.os && seen.target.same_file(asked))
    }

    /// The card for `record`: a repeat grows its count; a card not offered followed by an offered
    /// decision of the same target becomes that offered card under its own id (logged again, the
    /// reducer folds it into the one entry); a new one gets `mint`'s id, up to the cap.
    pub(super) fn card(
        &mut self,
        record: &Record<'_>,
        mint: impl FnOnce() -> Result<DenialId, Error>,
    ) -> Result<Recorded, Error> {
        let want = shape_of(record.kind);
        let held = self
            .cards
            .iter()
            .position(|card| shape_of(&card.kind) == want && card.target.same(record.card));
        if let Some(card) = held.and_then(|at| self.cards.get_mut(at)) {
            card.count = card.count.saturating_add(1);
            if record.offered && !card.offered {
                card.kind = record.kind.clone();
                card.target = record.card.clone();
                card.offered = true;
            }
            return Ok(Recorded::Log {
                denial: card.denial,
                count: card.count,
            });
        }
        if self.cards.len() >= MAX_CARDS_PER_TURN {
            let first = !self.capped_logged;
            self.capped_logged = true;
            return Ok(Recorded::Capped { first });
        }
        let denial = mint()?;
        self.cards.push(Card {
            kind: record.kind.clone(),
            target: record.card.clone(),
            offered: record.offered,
            denial,
            count: 1,
        });
        Ok(Recorded::Log { denial, count: 1 })
    }

    pub(super) fn target_of(&self, denial: DenialId) -> Option<&Target> {
        self.cards
            .iter()
            .find(|card| card.denial == denial)
            .map(|card| &card.target)
    }

    #[cfg(test)]
    pub(super) fn counts(&self) -> Vec<(DenialId, u32)> {
        self.cards
            .iter()
            .map(|card| (card.denial, card.count))
            .collect()
    }
}

/// A card's kind and its other matter, which a repeat must share.
fn shape_of(kind: &DenialKind) -> (u8, Option<MatterId>) {
    match kind {
        DenialKind::Folder { other_matter, .. } => (0, other_matter.as_ref().map(|o| o.matter)),
        DenialKind::File { other_matter, .. } => (1, other_matter.as_ref().map(|o| o.matter)),
        DenialKind::Site { .. } => (2, None),
        DenialKind::UpstreamAuth { .. } => (3, None),
        DenialKind::Unknown { .. } => (4, None),
    }
}
