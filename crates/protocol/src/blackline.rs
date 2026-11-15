//! The Word blackline's answer (`HostQuery::WordBlackline`, `QueryResult::WordBlackline`).
//!
//! It answers: what one Word document's text looks like between two versions, paragraph by
//! paragraph, as runs of text kept, inserted and deleted ([`WordBlackline`]); how each paragraph
//! changed as a whole ([`ParagraphChange`]); what each run is ([`RunKind`]); and what the
//! lawyer should know about the blackline as a whole ([`BlacklineNotice`], and the links whose
//! address changed, [`WordBlackline::changed_links`]): a part not shown, text shown that Word
//! hides, a change stetdoc's compare does not mark. The host makes it with stetdoc (`stetdoc
//! compare`, then `stetdoc view`, `stetdoc check`, and a view of each version); nothing in it is
//! a guess, and nothing is stored.

use serde::{Deserialize, Serialize};

use crate::open_enum::open_unit_enum;

/// A Word document's blackline, for `WordBlackline`: its body's paragraphs in document order,
/// each paragraph of a table cell one of its own, a table inside a cell included (its cells'
/// paragraphs come where the inner table stands). A struct, so later fields are additive; it
/// accepts fields it does not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WordBlackline {
    /// The paragraphs, first to last.
    pub paragraphs: Vec<BlacklineParagraph>,
    /// Whether stetdoc said its view stops before the end of the document: `paragraphs` are
    /// then the first part of it only.
    pub truncated: bool,
    /// What the lawyer should know about this blackline, each at most once, in the order of
    /// [`BlacklineNotice`]: a part of the document that could not be shown (the rest is), text
    /// shown that Word hides, numbers that may not match Word's. Empty when there is nothing to
    /// say; missing reads as empty, and empty is left out, so an answer from before the field
    /// still decodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<BlacklineNotice>,
    /// The text of each link whose web address differs between the two versions while its text
    /// does not (stetdoc 0.9.3's compare marks no change for it), each once, in the newer
    /// version's order: the app names each. Empty when none; missing reads as empty, and empty is
    /// left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_links: Vec<String>,
}

/// One paragraph of a blackline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlacklineParagraph {
    /// How the paragraph changed as a whole.
    pub change: ParagraphChange,
    /// Its text as runs, in order; two text runs next to each other never have the same kind,
    /// and only an object run is empty. An empty paragraph has none, and so does one
    /// `not_shown`.
    pub runs: Vec<BlacklineRun>,
    /// Whether it holds text that Word hides (or that is hard to see), which its runs show as
    /// ordinary text. False is left out, and missing reads as false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// A stretch of a paragraph's text with one kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlacklineRun {
    /// Kept, inserted or deleted.
    pub kind: RunKind,
    /// The text, as the document has it: a tab is `\t` and a line break `\n`. Empty for an
    /// object run.
    pub text: String,
    /// What the run stands for when a change holds no text of its own, such as an inserted
    /// picture or a deleted footnote reference: the app names it. Missing for a text run, and
    /// missing reads as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<BlacklineObject>,
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
        /// In both with the same text, but its formatting changed (a tracked format change,
        /// such as bold, struck through or hidden): every run is `same`.
        Formatted,
        /// A place Clauseo could not show: a line of stetdoc's view it could not read stands
        /// here, as a paragraph with no runs.
        NotShown,
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

open_unit_enum! {
    /// Something the lawyer should know about a blackline as a whole, said over it.
    pub enum BlacklineNotice {
        /// Part of the comparison could not be shown: a line of stetdoc's view Clauseo could not
        /// read, or content stetdoc said it does not show. The rest is shown.
        PartNotShown,
        /// The document holds text that Word hides or that is hard to see (white or tiny); the
        /// blackline shows it as ordinary text.
        HiddenText,
        /// Some list or clause numbers may not match the ones Word shows.
        NumbersMayDiffer,
        /// stetdoc lists tracked changes (by id, `stetdoc check`) or counts more (its summary)
        /// than the blackline's body names: the others are outside the main text (headers,
        /// footers, footnotes, endnotes), which it does not show.
        ChangesOutsideText,
        /// The text of a comment differs between the two versions; the blackline does not show
        /// comments, and stetdoc 0.9.3's compare marks no change for it.
        CommentChanged,
        /// The text in a text box changed; the blackline does not show the old text.
        TextBoxChanged,
        /// The formatting of some table cells changed (a tracked cell change), which the
        /// blackline does not mark.
        CellFormatChanged,
        /// stetdoc warned about something else this version does not know: some of the
        /// blackline may not match the document in Word exactly.
        MayDiffer,
    }
    test mod blackline_notice;
}

open_unit_enum! {
    /// What an inserted or deleted stretch with no text of its own stands for.
    pub enum BlacklineObject {
        /// A picture.
        Picture,
        /// A chart.
        Chart,
        /// A shape, or a text box with no text.
        Shape,
        /// A footnote's or an endnote's reference.
        Note,
        /// A field, such as a page number or a date Word fills in.
        Field,
        /// Anything else: a link, a comment's anchor, a content control, an embedded object, or a
        /// change that held nothing at all.
        Other,
    }
    test mod blackline_object;
}
