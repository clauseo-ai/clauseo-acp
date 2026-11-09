//! The Word blackline's answer (`HostQuery::WordBlackline`, `QueryResult::WordBlackline`).
//!
//! It answers: what one Word document's text looks like between two versions, paragraph by
//! paragraph, as runs of text kept, inserted and deleted ([`WordBlackline`]); how each paragraph
//! changed as a whole ([`ParagraphChange`]); and what each run is ([`RunKind`]). The host makes
//! it with stetdoc (`stetdoc compare`, then `stetdoc view`); nothing in it is a guess, and
//! nothing is stored.

use serde::{Deserialize, Serialize};

use crate::open_enum::open_unit_enum;

/// A Word document's blackline, for `WordBlackline`: its body's paragraphs in document order,
/// each table cell a paragraph of its own. A struct, so later fields are additive; it accepts
/// fields it does not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WordBlackline {
    /// The paragraphs, first to last.
    pub paragraphs: Vec<BlacklineParagraph>,
    /// Whether stetdoc said its view stops before the end of the document: `paragraphs` are
    /// then the first part of it only.
    pub truncated: bool,
}

/// One paragraph of a blackline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlacklineParagraph {
    /// How the paragraph changed as a whole.
    pub change: ParagraphChange,
    /// Its text as runs, in order; two runs next to each other never have the same kind, and
    /// no run is empty. An empty paragraph has none.
    pub runs: Vec<BlacklineRun>,
}

/// A stretch of a paragraph's text with one kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlacklineRun {
    /// Kept, inserted or deleted.
    pub kind: RunKind,
    /// The text, as the document has it: a tab is `\t` and a line break `\n`.
    pub text: String,
}

open_unit_enum! {
    /// How a paragraph changed between the two versions.
    pub enum ParagraphChange {
        /// The same in both: every run is `same`.
        Same,
        /// In both, with text inserted or deleted.
        Changed,
        /// Only in the newer version: the whole paragraph was inserted.
        Added,
        /// Only in the older version: the whole paragraph was deleted.
        Removed,
    }
    test mod paragraph_change;
}

open_unit_enum! {
    /// What a run of a blackline paragraph is.
    pub enum RunKind {
        /// Text in both versions.
        Same,
        /// Text only in the newer version.
        Ins,
        /// Text only in the older version.
        Del,
    }
    test mod run_kind;
}
