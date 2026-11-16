//! A .docx read for its tables' formatting, for `tables.rs`: the zip opened, the parts found by
//! the package's relationships, and each part's tables streamed with quick-xml (`NsReader`,
//! elements matched in Word's main namespace, Transitional or Strict, whatever the prefix).
//! Pure: it reads bytes in memory, never a file.
//!
//! It answers:
//! - which parts are read: the main document (`_rels/.rels`' `officeDocument`, else
//!   `word/document.xml`), and the headers, footers, footnotes and endnotes its relationships
//!   name; a header or footer no section uses is left out, as Word shows it nowhere;
//! - where a header or footer is ([`HeaderUse`]): each section's `w:headerReference` and
//!   `w:footerReference` by kind of page, a section with none for a kind taking the one before
//!   it, as Word does;
//! - what a table holds: its `w:tblPr` and `w:tblGrid`, each row's `w:trPr` and `w:tblPrEx`, and
//!   each cell's `w:tcPr`, every property element as one canonical string (its name, its
//!   attributes sorted, its children), leaving out the noise: `rsid` attributes, `w14:paraId`
//!   and `w14:textId`, a tracked change's old properties (`*PrChange`) and the tracked marks
//!   (`w:ins`, `w:del`, `w:cellIns`, `w:cellDel`, `w:cellMerge`); a row or cell whose deletion is
//!   tracked is left out, as accepting it does; and the text of each table, row and cell (its
//!   `w:t`), by which they are paired;
//! - what can't be read: a zip that does not open, a part missing, over [`MAX_PART_BYTES`] or
//!   [`MAX_ALL_BYTES`] in all, or not well-formed XML, gives no package (`None`), never a guess.

use std::collections::BTreeMap;
use std::io::{Cursor, Read as _};

use clauseo_protocol::{HeaderSlot, HeaderUse};
use quick_xml::events::Event;

use part::tables_of;

mod part;

/// The most bytes of one XML part read, decompressed.
pub(crate) const MAX_PART_BYTES: u64 = 16 * 1024 * 1024;

/// The most bytes one file's read takes, decompressed, over all its parts.
pub(crate) const MAX_ALL_BYTES: u64 = 64 * 1024 * 1024;

/// Word's main XML namespaces, Transitional and Strict.
const W_NAMESPACES: &[&str] = &[
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
];

/// The markup-compatibility namespace, whose `Fallback` repeats what its `Choice` holds.
const MC_NAMESPACE: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// The tracked marks a property element may hold, which are not formatting.
const MARKS: &[&str] = &["ins", "del", "cellIns", "cellDel", "cellMerge"];

/// One property element: its local name and its canonical form.
pub(crate) type Prop = (String, String);

/// A table: its text, its own properties and grid, and its rows.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Table {
    pub(crate) text: String,
    pub(crate) props: Vec<Prop>,
    pub(crate) grid: Vec<Prop>,
    pub(crate) rows: Vec<Row>,
}

/// A row: its text, its properties (`w:trPr`'s and `w:tblPrEx`'s children) and its cells.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) text: String,
    pub(crate) props: Vec<Prop>,
    pub(crate) cells: Vec<Cell>,
    deleted: bool,
}

/// A cell: its text and its properties.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Cell {
    pub(crate) text: String,
    pub(crate) props: Vec<Prop>,
    deleted: bool,
}

/// Which part of the package a list of tables is in, and where a header or footer is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Where {
    Body,
    Header(Vec<HeaderUse>),
    Footer(Vec<HeaderUse>),
    Footnotes,
    Endnotes,
}

/// A package read: each part's tables, in the order of [`Where`] (headers and footers each by
/// their first use).
#[derive(Debug, Default)]
pub(crate) struct Package {
    pub(crate) parts: Vec<(Where, Vec<Table>)>,
}

/// One relationship: its type's last word and its target, resolved to a part's name.
struct Rel {
    id: String,
    kind: String,
    target: String,
}

/// One section's header and footer references: footer or not, the kind of page, the id.
type SectionRefs = Vec<(bool, HeaderSlot, String)>;

/// The package in `bytes`, or `None` when it can't be read whole (the module doc).
pub(crate) fn read_package(bytes: &[u8]) -> Option<Package> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut left = MAX_ALL_BYTES;
    let main = match read_part(&mut zip, "_rels/.rels", &mut left) {
        Some(rels) => rels_of(&rels, "")?
            .into_iter()
            .find(|rel| rel.kind == "officeDocument")
            .map_or_else(|| "word/document.xml".to_owned(), |rel| rel.target),
        None => "word/document.xml".to_owned(),
    };
    let (dir, file) = main.rsplit_once('/').unwrap_or(("", main.as_str()));
    let rels_name = if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    };
    let rels = match read_part(&mut zip, &rels_name, &mut left) {
        Some(xml) => rels_of(&xml, dir)?,
        None => Vec::new(),
    };
    let document = read_part(&mut zip, &main, &mut left)?;
    let (body, sections) = tables_of(&document, true)?;
    let mut package = Package::default();
    package.parts.push((Where::Body, body));
    let mut headers: Vec<((u32, u8), Where, Vec<Table>)> = Vec::new();
    let mut uses = uses_of(&sections, &rels);
    for rel in &rels {
        let place = match rel.kind.as_str() {
            // Taken out once read, so a part two relationships name is read once.
            "header" | "footer" => match uses.remove(&rel.target) {
                Some(used) => used,
                None => continue,
            },
            "footnotes" => Where::Footnotes,
            "endnotes" => Where::Endnotes,
            _ => continue,
        };
        let xml = read_part(&mut zip, &rel.target, &mut left)?;
        let (tables, _) = tables_of(&xml, false)?;
        match place {
            Where::Header(_) | Where::Footer(_) => headers.push((first_use(&place), place, tables)),
            notes @ (Where::Body | Where::Footnotes | Where::Endnotes) => {
                package.parts.push((notes, tables));
            }
        }
    }
    headers.sort_by_key(|(first, _, _)| *first);
    package.parts.extend(
        headers
            .into_iter()
            .map(|(_, place, tables)| (place, tables)),
    );
    package.parts.sort_by_key(|(place, _)| rank(place));
    Some(package)
}

/// Where a part goes: the body, the footnotes, the endnotes, then headers and footers.
fn rank(place: &Where) -> u8 {
    match place {
        Where::Body => 0,
        Where::Footnotes => 1,
        Where::Endnotes => 2,
        Where::Header(_) => 3,
        Where::Footer(_) => 4,
    }
}

/// A header's or footer's first use: its first section and kind of page.
pub(crate) fn first_use(place: &Where) -> (u32, u8) {
    let uses = match place {
        Where::Header(uses) | Where::Footer(uses) => uses.as_slice(),
        Where::Body | Where::Footnotes | Where::Endnotes => &[],
    };
    uses.iter()
        .flat_map(|used| {
            let slot = slot_rank(used.slot);
            used.sections.iter().map(move |&section| (section, slot))
        })
        .min()
        .unwrap_or((u32::MAX, u8::MAX))
}

/// The order of the kinds of page.
fn slot_rank(slot: HeaderSlot) -> u8 {
    match slot {
        HeaderSlot::Default => 0,
        HeaderSlot::FirstPage => 1,
        HeaderSlot::EvenPages | HeaderSlot::Unknown => 2,
    }
}

/// Where each header and footer part is, by its name: each section's references by kind of
/// page, a section with none for a kind taking the one before it.
fn uses_of(sections: &[SectionRefs], rels: &[Rel]) -> BTreeMap<String, Where> {
    let slots = [
        HeaderSlot::Default,
        HeaderSlot::FirstPage,
        HeaderSlot::EvenPages,
    ];
    let mut found: BTreeMap<String, (bool, Vec<HeaderUse>)> = BTreeMap::new();
    for footer in [false, true] {
        for slot in slots {
            let mut current: Option<&str> = None;
            for (index, refs) in sections.iter().enumerate() {
                if let Some((_, _, id)) = refs.iter().find(|r| r.0 == footer && r.1 == slot) {
                    current = Some(id);
                }
                let Some(target) = current
                    .and_then(|id| rels.iter().find(|rel| rel.id == id))
                    .map(|rel| rel.target.clone())
                else {
                    continue;
                };
                let section = u32::try_from(index + 1).unwrap_or(u32::MAX);
                let entry = found.entry(target).or_insert_with(|| (footer, Vec::new()));
                match entry.1.iter_mut().find(|used| used.slot == slot) {
                    Some(used) => used.sections.push(section),
                    None => entry.1.push(HeaderUse {
                        slot,
                        sections: vec![section],
                    }),
                }
            }
        }
    }
    found
        .into_iter()
        .map(|(name, (footer, uses))| {
            let place = if footer {
                Where::Footer(uses)
            } else {
                Where::Header(uses)
            };
            (name, place)
        })
        .collect()
}

/// The part `name` whole, at most [`MAX_PART_BYTES`] and `left`, which the read takes from;
/// `None` when it is missing, past a cap or does not read.
fn read_part<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    left: &mut u64,
) -> Option<Vec<u8>> {
    let part = zip.by_name(name).ok()?;
    let limit = MAX_PART_BYTES.min(*left);
    let mut bytes = Vec::new();
    part.take(limit + 1).read_to_end(&mut bytes).ok()?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if read > limit {
        return None;
    }
    *left -= read;
    Some(bytes)
}

/// The relationships in `xml`, a `.rels` part of a part in the folder `dir`; `None` when it does
/// not read. External targets are left out.
fn rels_of(xml: &[u8], dir: &str) -> Option<Vec<Rel>> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut rels = Vec::new();
    loop {
        buf.clear();
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(element) | Event::Empty(element)
                if element.local_name().as_ref() == "Relationship" =>
            {
                let attr = |key: &str| {
                    element
                        .attributes()
                        .flatten()
                        .find(|attr| attr.key.local_name().as_ref() == key)
                        .map(|attr| attr.value.to_string())
                };
                if attr("TargetMode").is_some_and(|mode| mode == "External") {
                    continue;
                }
                let (Some(id), Some(kind), Some(target)) =
                    (attr("Id"), attr("Type"), attr("Target"))
                else {
                    continue;
                };
                let kind = kind.rsplit('/').next().unwrap_or_default().to_owned();
                rels.push(Rel {
                    id,
                    kind,
                    target: resolve(dir, &target),
                });
            }
            Event::Eof => return Some(rels),
            Event::Start(_)
            | Event::End(_)
            | Event::Empty(_)
            | Event::Text(_)
            | Event::CData(_)
            | Event::Comment(_)
            | Event::Decl(_)
            | Event::PI(_)
            | Event::DocType(_)
            | Event::GeneralRef(_) => {}
        }
    }
}

/// A relationship's `target` as a part's name, from the folder `dir` of its source.
fn resolve(dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|part| !part.is_empty()).collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}
