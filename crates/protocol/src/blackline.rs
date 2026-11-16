//! The Word blackline's answer (`HostQuery::WordBlackline`, `QueryResult::WordBlackline`).
//!
//! It answers: what one Word document's text looks like between two versions, paragraph by
//! paragraph, as runs of text kept, inserted and deleted ([`WordBlackline`]); how each paragraph
//! changed as a whole ([`ParagraphChange`]); what each run is ([`RunKind`]); and what the
//! lawyer should know about the blackline as a whole ([`BlacklineNotice`], the links whose
//! address changed, [`WordBlackline::changed_links`], and the tables whose formatting changed,
//! [`WordBlackline::changed_tables`]): a part not shown, text shown that Word hides, a change
//! stetdoc's compare does not mark. The footnotes, endnotes, headers and footers that hold a
//! change come with their paragraphs ([`WordBlackline::parts`], [`BlacklinePart`]). The host
//! makes it with stetdoc (`stetdoc compare`, then `stetdoc view` of the body, the notes and the
//! headers, `stetdoc check`, and a view of each version) and its own comparison of the two
//! files' table formatting; nothing in it is a guess, and nothing is stored.

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};

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
    /// The footnotes, endnotes, headers and footers that hold a change, each with its
    /// paragraphs: the footnotes by number, then the endnotes by number, then the headers and
    /// then the footers, each in the order of the first section they serve. One that holds no
    /// change is left out. Empty when none; missing reads as empty, and empty is left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<BlacklinePart>,
    /// The tables whose formatting differs between the two versions (stetdoc 0.9.3's compare
    /// marks none of it), found by Clauseo's own comparison of the two files; each once, in the
    /// order of [`TablePlace`], then by number. At most 50: past them the blackline says it may
    /// differ. Empty when none; missing reads as empty, and empty is left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_tables: Vec<ChangedTable>,
    /// How many tables' formatting differs, those past the 50 [`Self::changed_tables`] holds
    /// counted too: more than that list's length only past the cap. 0 when none; missing reads
    /// as 0, and 0 is left out.
    #[serde(default, skip_serializing_if = "is_default")]
    pub changed_tables_total: u32,
}

/// Whether `value` is its type's default, for a field left out when it is.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// A footnote, an endnote, a header or a footer that holds a change, shown after the body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlacklinePart {
    /// Which one it is.
    pub kind: BlacklinePartKind,
    /// Its paragraphs, first to last, as the body's are (a table's cells' included).
    pub paragraphs: Vec<BlacklineParagraph>,
}

open_data_enum! {
    /// Which footnote, endnote, header or footer a [`BlacklinePart`] is.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum BlacklinePartKind {
        /// A footnote, by the number Word shows for it.
        Footnote = "footnote" {
            /// Its number, from 1.
            number: u32,
        },
        /// An endnote, by the number Word shows for it.
        Endnote = "endnote" {
            /// Its number, from 1.
            number: u32,
        },
        /// A header, with the pages it is on.
        Header = "header" {
            /// Where Word shows it: each kind of page, with the sections. Empty for a part no
            /// section uses (stetdoc prints a bare `{header}`).
            uses: Vec<HeaderUse>,
        },
        /// A footer, with the pages it is on.
        Footer = "footer" {
            /// Where Word shows it, as for a header.
            uses: Vec<HeaderUse>,
        },
        _ =>
        /// A kind from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod blackline_part_kind with [
        BlacklinePartKind::Footnote { number: 3 },
        BlacklinePartKind::Endnote { number: 1 },
        BlacklinePartKind::Header {
            uses: vec![HeaderUse {
                slot: HeaderSlot::Default,
                sections: vec![1, 2],
            }],
        },
        BlacklinePartKind::Footer {
            uses: vec![HeaderUse {
                slot: HeaderSlot::FirstPage,
                sections: vec![2],
            }],
        },
    ];
}

/// One kind of page a header or footer is on, and the sections where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HeaderUse {
    /// The kind of page.
    pub slot: HeaderSlot,
    /// The sections, from 1, in order; never empty.
    pub sections: Vec<u32>,
}

open_unit_enum! {
    /// Which pages of a section a header or footer is on.
    pub enum HeaderSlot {
        /// Every page the others do not take.
        Default,
        /// The section's first page.
        FirstPage,
        /// The even pages.
        EvenPages,
    }
    test mod header_slot;
}

/// A table whose formatting differs between the two versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ChangedTable {
    /// The part of the document it is in.
    pub place: TablePlace,
    /// Its number in that part, from 1, counting every table in the order it starts (a table
    /// inside a cell counts too), in the newer version.
    pub number: u32,
    /// What changed, each once, in the order of [`TableFormat`]; never empty.
    pub formats: Vec<TableFormat>,
}

open_data_enum! {
    /// The part of the document a [`ChangedTable`] is in.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum TablePlace {
        /// The body.
        Body = "body",
        /// A header, with the pages it is on.
        Header = "header" {
            /// Where Word shows it, in the newer version. Empty when it is on none.
            uses: Vec<HeaderUse>,
        },
        /// A footer, with the pages it is on.
        Footer = "footer" {
            /// Where Word shows it, as for a header.
            uses: Vec<HeaderUse>,
        },
        /// The footnotes, all together: the number counts the tables of every footnote.
        Footnotes = "footnotes",
        /// The endnotes, all together.
        Endnotes = "endnotes",
        _ =>
        /// A place from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod table_place with [
        TablePlace::Body,
        TablePlace::Header {
            uses: vec![HeaderUse {
                slot: HeaderSlot::Default,
                sections: vec![1],
            }],
        },
        TablePlace::Footer { uses: Vec::new() },
        TablePlace::Footnotes,
        TablePlace::Endnotes,
    ];
}

open_unit_enum! {
    /// What changed in a table's formatting: the table's own, a row's or a cell's.
    pub enum TableFormat {
        /// Borders.
        Borders,
        /// Shading.
        Shading,
        /// Widths: the table's, its columns', a cell's, or the table's indent.
        Widths,
        /// Alignment: the table's on the page, a row's, or a cell's text from top to bottom.
        Alignment,
        /// Merged cells.
        MergedCells,
        /// A row's height.
        RowHeight,
        /// Anything else: the table's style, cell margins, a row kept on one page or repeated as
        /// a heading, and the like.
        Other,
    }
    test mod table_format;
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
        /// than the blackline shows, in its body and its parts: the others are outside what it
        /// shows (a comment, or a footnote, endnote, header or footer it could not show).
        ChangesOutsideText,
        /// A header or footer was not compared between the two versions: one version has it and
        /// the other does not, or both have one but stetdoc's compare found no one-to-one partner
        /// for it, or it is on a section the other version lacks. The blackline does not show
        /// what changed in it.
        HeaderOrFooterNotCompared,
        /// The text of a comment differs between the two versions; the blackline does not show
        /// comments, and stetdoc 0.9.3's compare marks no change for it.
        CommentChanged,
        /// The text in a text box changed; the blackline does not show the old text.
        TextBoxChanged,
        /// The formatting of some tables changed: a table's own (its borders, widths, alignment
        /// and the like), a row's or a cell's (its shading, borders, merging). The blackline
        /// does not mark it. [`WordBlackline::changed_tables`] names each table Clauseo's own
        /// comparison of the two files found. The notice can also come from stetdoc's list of
        /// changes (a tracked cell change in the redline), which names no table.
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
