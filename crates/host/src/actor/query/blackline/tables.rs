//! The tables' formatting of a Word blackline's two versions, compared by Clauseo itself:
//! stetdoc 0.9.3's compare marks no change to a table's own formatting, a row's or a cell's
//! (`w:tblPr`, `w:tblGrid`, `w:trPr`, `w:tcPr`: borders, shading, widths, alignment, merged
//! cells, row height), so the blackline would miss it. A pure function over the two files'
//! bytes ([`compare`]), run on the blackline's thread (`run.rs`); no I/O.
//!
//! It answers:
//! - what is read (`read.rs`): each version's body, headers, footers, footnotes and endnotes,
//!   their tables' property elements with the noise left out;
//! - which parts are compared: the body with the body, the footnotes with the footnotes, the
//!   endnotes with the endnotes, and the headers and footers about as stetdoc's compare pairs
//!   them ([`pair_headers`]: the sections from the last one back, and in each pair of sections
//!   the part on the same kind of page; an approximation). A header or footer with tables and no
//!   counterpart may differ, since what changed in its tables can't be told, and so may any
//!   header or footer with tables when the versions have different numbers of sections;
//! - which tables, rows and cells are paired ([`align`]): within a part (a table's rows, a row's
//!   cells), by order when both versions have as many; otherwise by their text, the longest
//!   run of equal texts in order, then the ones between two pairs by order where both sides
//!   have as many there. One left between two pairs on one side only was inserted or deleted
//!   whole, which the blackline shows;
//! - what changed: a paired table's own properties, its grid, and each paired row's and cell's,
//!   compared element by element, each difference named by its group ([`group_of`]);
//! - which headers and footers were not compared at all: the texts of the parts one version has
//!   and the other does not ([`TablesCompared::lone_header_texts`]); stetdoc's compare does not
//!   compare a header of a section it pairs with none, and does not warn of it, so `run.rs`
//!   says so when no part the blackline shows has that text, and always for such a part with
//!   no text or a picture ([`TablesCompared::lone_header_unseen`]). The parts are counted, so
//!   two parts with one text are not taken for one;
//! - what is never silent: where both sides have tables, rows or cells left between two pairs
//!   and not as many of them, what changed there can't be put on a table, so the part may differ
//!   ([`TablesCompared::may_differ`]); and so may the whole document when a version can't be
//!   read (a zip that does not open, a part missing, past a cap or not well-formed). Past
//!   `run.rs`'s cap on the tables named, the rest may differ too.
//!
//! A table is named by its part and its number in it in the newer version, from 1, counting
//! every table in the order it starts, a table inside a cell too.

use clauseo_protocol::{ChangedTable, TableFormat, TablePlace};

use std::collections::{BTreeMap, BTreeSet};

use read::{Package, Prop, Table, Where, read_package};

mod read;

/// The most cells the text alignment of one list may fill (its old length times its new): past
/// it the list is not aligned, and what is left unpaired may differ.
const ALIGN_MAX: usize = 4_000_000;

/// What the comparison found: the tables whose formatting changed, and whether something may
/// differ that it could not put on a table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TablesCompared {
    /// The changed tables, in the order of the parts, then by number; every one, uncapped.
    pub(crate) changed: Vec<ChangedTable>,
    /// Whether the blackline may differ (the module doc).
    pub(crate) may_differ: bool,
    /// The texts of the headers and footers some section uses that only one version has, each
    /// with its white space taken out ([`squeezed`]), once, sorted. The parts are counted, so a
    /// text one version has on more parts than the other is listed too. A part with no text or
    /// with a picture is not listed: it sets [`TablesCompared::lone_header_unseen`] instead.
    pub(crate) lone_header_texts: Vec<String>,
    /// Whether a header or footer only one version has holds no text or a picture (a letterhead
    /// logo): no shown text can stand for it, so `run.rs` says it was not compared.
    pub(crate) lone_header_unseen: bool,
}

/// The tables' formatting of `old` and `new`, two .docx files' bytes, compared.
pub(crate) fn compare(old: &[u8], new: &[u8]) -> TablesCompared {
    let (Some(old), Some(new)) = (read_package(old), read_package(new)) else {
        return TablesCompared {
            may_differ: true,
            ..TablesCompared::default()
        };
    };
    let mut found = TablesCompared::default();
    for (place, new_tables) in &new.parts {
        let Some((_, old_tables)) = old.parts.iter().find(|(theirs, _)| theirs == place) else {
            continue;
        };
        compare_part(place, old_tables, new_tables, &mut found);
    }
    let headers = pair_headers(&old, &new);
    for (o, n) in &headers.pairs {
        if let (Some(old_part), Some(new_part)) = (old.headers.get(*o), new.headers.get(*n)) {
            compare_part(
                &new_part.place,
                &old_part.tables,
                &new_part.tables,
                &mut found,
            );
        }
    }
    // A header or footer with tables and no counterpart: what changed in them can't be told.
    let alone = |package: &Package, matched: &BTreeSet<&str>| {
        package
            .headers
            .iter()
            .any(|(name, part)| !part.tables.is_empty() && !matched.contains(name.as_str()))
    };
    found.may_differ |= alone(&old, &headers.old) || alone(&new, &headers.new);
    // The pairing only comes near stetdoc's when the versions have as many sections: with a
    // section added or removed, a header with tables may pair with the wrong one.
    let tabled = |package: &Package| package.headers.values().any(|part| !part.tables.is_empty());
    found.may_differ |= old.sections.len() != new.sections.len() && (tabled(&old) || tabled(&new));
    // Each header and footer part by its text and whether it holds a picture, counted: the
    // newer version's count less the older's. A part one version has more of is lone.
    let mut counts: BTreeMap<(String, bool), i64> = BTreeMap::new();
    for (package, step) in [(&old, -1), (&new, 1)] {
        for part in package.headers.values() {
            *counts
                .entry((squeezed(&part.text), part.picture))
                .or_default() += step;
        }
    }
    let mut lone = BTreeSet::new();
    for ((text, picture), count) in counts {
        if count == 0 {
            continue;
        }
        if text.is_empty() || picture {
            found.lone_header_unseen = true;
        } else {
            lone.insert(text);
        }
    }
    found.lone_header_texts = lone.into_iter().collect();
    // The body, the notes, then the headers and the footers; each part's tables by number.
    found.changed.sort_by_key(|table| place_rank(&table.place));
    found
}

/// Compares the tables of one part, `old_tables` and `new_tables`, into `found`, each changed
/// table named at `place`.
fn compare_part(
    place: &Where,
    old_tables: &[Table],
    new_tables: &[Table],
    found: &mut TablesCompared,
) {
    let (pairs, unpaired) = align_by_text(old_tables, new_tables, |table| &table.text);
    found.may_differ |= unpaired;
    for (o, n) in pairs {
        let (Some(old_table), Some(new_table)) = (old_tables.get(o), new_tables.get(n)) else {
            continue;
        };
        let mut formats = Vec::new();
        found.may_differ |= compare_table(old_table, new_table, &mut formats);
        if !formats.is_empty() {
            found.changed.push(ChangedTable {
                place: place_of(place),
                number: u32::try_from(n + 1).unwrap_or(u32::MAX),
                formats: TableFormat::ALL
                    .into_iter()
                    .filter(|format| formats.contains(format))
                    .collect(),
            });
        }
    }
}

/// The header and footer parts paired across the versions, and the ones matched on each side.
#[derive(Debug, Default)]
struct HeaderPairs<'a> {
    /// The pairs (older part's name, newer part's name), in the order of the newer part's first
    /// use.
    pairs: Vec<(&'a str, &'a str)>,
    old: BTreeSet<&'a str>,
    new: BTreeSet<&'a str>,
}

/// Pairs the versions' header and footer parts the way stetdoc's compare comes near to: the
/// sections from the last one back, and in each pair of sections the parts on the same kind of
/// page. Each part pairs once; a later pairing that names a part already paired is left out.
/// It is an approximation: stetdoc pairs the two last sections, and two others only when their
/// ending paragraphs were paired in the body, so a section added at the end or in the middle
/// pairs differently there. [`compare`] says the blackline may differ when the versions have
/// different numbers of sections and a header or footer holds tables. As many sections on each
/// side can still pair wrongly (one section added and another removed), and nothing says so;
/// only the table formatting compared here is affected, since the header texts are checked
/// against the parts shown whatever the pairing.
fn pair_headers<'a>(old: &'a Package, new: &'a Package) -> HeaderPairs<'a> {
    let mut found = HeaderPairs::default();
    for (theirs, ours) in old.sections.iter().rev().zip(new.sections.iter().rev()) {
        for (footer, slot, name) in ours {
            let Some((_, _, counterpart)) = theirs
                .iter()
                .find(|(other, kind, _)| other == footer && kind == slot)
            else {
                continue;
            };
            if found.old.contains(counterpart.as_str()) || found.new.contains(name.as_str()) {
                continue;
            }
            found.old.insert(counterpart.as_str());
            found.new.insert(name.as_str());
            found.pairs.push((counterpart.as_str(), name.as_str()));
        }
    }
    found.pairs.sort_by_key(|(_, name)| {
        new.headers
            .get(*name)
            .map_or((u32::MAX, u8::MAX), |part| read::first_use(&part.place))
    });
    found
}

/// The protocol's place of a part.
fn place_of(place: &Where) -> TablePlace {
    match place {
        Where::Body => TablePlace::Body,
        Where::Header(uses) => TablePlace::Header { uses: uses.clone() },
        Where::Footer(uses) => TablePlace::Footer { uses: uses.clone() },
        Where::Footnotes => TablePlace::Footnotes,
        Where::Endnotes => TablePlace::Endnotes,
    }
}

/// The order of the places: the body, the footnotes, the endnotes, the headers, the footers.
/// The sort is stable, so the headers keep the order of their first use.
fn place_rank(place: &TablePlace) -> u8 {
    match place {
        TablePlace::Body => 0,
        TablePlace::Footnotes => 1,
        TablePlace::Endnotes => 2,
        TablePlace::Header { .. } => 3,
        TablePlace::Footer { .. } => 4,
        TablePlace::Unknown { .. } => 5,
    }
}

/// Compares two paired tables into `formats`: whether something may differ that can't be put
/// on it (rows, or a row's cells, left unpaired on both sides).
fn compare_table(old: &Table, new: &Table, formats: &mut Vec<TableFormat>) -> bool {
    compare_props(&old.props, &new.props, formats);
    if old.grid != new.grid {
        add(formats, TableFormat::Widths);
    }
    let (rows, mut unpaired) = align_by_text(&old.rows, &new.rows, |row| &row.text);
    for (o, n) in rows {
        let (Some(old_row), Some(new_row)) = (old.rows.get(o), new.rows.get(n)) else {
            continue;
        };
        compare_props(&old_row.props, &new_row.props, formats);
        let (cells, cells_unpaired) =
            align_by_text(&old_row.cells, &new_row.cells, |cell| &cell.text);
        unpaired |= cells_unpaired;
        for (o, n) in cells {
            if let (Some(old_cell), Some(new_cell)) = (old_row.cells.get(o), new_row.cells.get(n)) {
                compare_props(&old_cell.props, &new_cell.props, formats);
            }
        }
    }
    unpaired
}

/// Adds to `formats` the group of each property element whose forms differ between `old` and
/// `new`, the element missing on one side included.
fn compare_props(old: &[Prop], new: &[Prop], formats: &mut Vec<TableFormat>) {
    for (name, _) in old.iter().chain(new) {
        if forms(old, name) != forms(new, name) {
            add(formats, group_of(name));
        }
    }
}

/// The forms of the elements named `name` in `props`, sorted.
fn forms<'a>(props: &'a [Prop], name: &str) -> Vec<&'a str> {
    let mut forms: Vec<&str> = props
        .iter()
        .filter(|(theirs, _)| theirs == name)
        .map(|(_, form)| form.as_str())
        .collect();
    forms.sort_unstable();
    forms
}

/// `text` with its white space taken out, so a header's text read from its part and the one a
/// story view shows (with a tab, a line break) compare equal.
pub(crate) fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Adds `format` to `formats` once.
fn add(formats: &mut Vec<TableFormat>, format: TableFormat) {
    if !formats.contains(&format) {
        formats.push(format);
    }
}

/// The group a table's, row's or cell's property element is in, by its local name.
pub(crate) fn group_of(name: &str) -> TableFormat {
    match name {
        "tblBorders" | "tcBorders" => TableFormat::Borders,
        "shd" => TableFormat::Shading,
        "tblW" | "tcW" | "tblInd" | "tblLayout" | "gridCol" | "wBefore" | "wAfter"
        | "gridBefore" | "gridAfter" => TableFormat::Widths,
        "jc" | "vAlign" => TableFormat::Alignment,
        "gridSpan" | "vMerge" | "hMerge" => TableFormat::MergedCells,
        "trHeight" => TableFormat::RowHeight,
        _ => TableFormat::Other,
    }
}

/// [`align`] on the texts `text` gives `old` and `new`.
fn align_by_text<T>(
    old: &[T],
    new: &[T],
    text: impl Fn(&T) -> &String,
) -> (Vec<(usize, usize)>, bool) {
    let old: Vec<&str> = old.iter().map(|item| text(item).as_str()).collect();
    let new: Vec<&str> = new.iter().map(|item| text(item).as_str()).collect();
    align(&old, &new)
}

/// The pairs of indexes of `old` and `new` (the module doc: by order when as many, else by
/// text), and whether items are left unpaired on both sides between two pairs.
pub(crate) fn align(old: &[&str], new: &[&str]) -> (Vec<(usize, usize)>, bool) {
    if old.len() == new.len() {
        return ((0..old.len()).map(|i| (i, i)).collect(), false);
    }
    if old.is_empty() || new.is_empty() {
        return (Vec::new(), false);
    }
    if old.len().saturating_mul(new.len()) > ALIGN_MAX {
        return (Vec::new(), true);
    }
    // The longest run of equal texts in order: `longest[i][j]` for `old[i..]` and `new[j..]`.
    let width = new.len() + 1;
    let mut longest = vec![0_u32; (old.len() + 1) * width];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            let at = |i: usize, j: usize| longest.get(i * width + j).copied().unwrap_or(0);
            let value = if old.get(i) == new.get(j) {
                at(i + 1, j + 1) + 1
            } else {
                at(i + 1, j).max(at(i, j + 1))
            };
            if let Some(cell) = longest.get_mut(i * width + j) {
                *cell = value;
            }
        }
    }
    let at = |i: usize, j: usize| longest.get(i * width + j).copied().unwrap_or(0);
    let mut anchors = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old.get(i) == new.get(j) {
            anchors.push((i, j));
            i += 1;
            j += 1;
        } else if at(i + 1, j) >= at(i, j + 1) {
            i += 1;
        } else {
            j += 1;
        }
    }
    anchors.push((old.len(), new.len()));
    let mut pairs = Vec::new();
    let mut unpaired = false;
    let (mut from_old, mut from_new) = (0, 0);
    for (to_old, to_new) in anchors {
        let (gap_old, gap_new) = (to_old - from_old, to_new - from_new);
        if gap_old == gap_new {
            pairs.extend((0..gap_old).map(|k| (from_old + k, from_new + k)));
        } else if gap_old > 0 && gap_new > 0 {
            unpaired = true;
        }
        if to_old < old.len() {
            pairs.push((to_old, to_new));
        }
        (from_old, from_new) = (to_old + 1, to_new + 1);
    }
    (pairs, unpaired)
}

#[cfg(test)]
mod tests;
