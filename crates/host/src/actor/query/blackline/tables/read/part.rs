//! One XML part of a .docx streamed for its tables, for `read.rs`: each table, row and cell
//! with its property elements in canonical form and its text, and the main document's
//! sections' header and footer references (the rules are `read.rs`'s module doc).

use std::fmt::Write as _;
use std::io::BufRead;

use clauseo_protocol::HeaderSlot;
use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};

use super::{Cell, MARKS, MC_NAMESPACE, Prop, Row, SectionRefs, Table, W_NAMESPACES};

/// What an open element is, to the streaming read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Table,
    Row,
    Cell,
    Text,
    Other,
}

/// What one XML part holds: its tables, in the order they start, (for the main document,
/// `main`) its sections' references, and its whole text.
type PartRead = (Vec<Table>, Vec<SectionRefs>, String);

/// One XML part read ([`PartRead`]); `None` when it does not read.
pub(super) fn tables_of(xml: &[u8], main: bool) -> Option<PartRead> {
    let mut reader = NsReader::from_reader(xml);
    let mut buf = Vec::new();
    let mut tables: Vec<Table> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut sections: Vec<SectionRefs> = Vec::new();
    let mut whole = String::new();
    loop {
        buf.clear();
        let (resolved, event) = reader.read_resolved_event_into(&mut buf).ok()?;
        let word =
            matches!(resolved, ResolveResult::Bound(Namespace(ns)) if W_NAMESPACES.contains(&ns));
        let fallback =
            matches!(resolved, ResolveResult::Bound(Namespace(ns)) if ns == MC_NAMESPACE);
        match event {
            Event::Start(element) => {
                let name = element.local_name();
                let name = if word { name.as_ref() } else { "" };
                let top = stack.last().copied();
                let table = open.last().copied();
                match (name, top, table) {
                    ("tblPr", Some(Frame::Table), Some(t)) => {
                        let props = capture(&mut reader)?;
                        tables
                            .get_mut(t)?
                            .props
                            .extend(props.into_iter().map(|p| p.prop));
                    }
                    ("tblGrid", Some(Frame::Table), Some(t)) => {
                        let props = capture(&mut reader)?;
                        tables
                            .get_mut(t)?
                            .grid
                            .extend(props.into_iter().map(|p| p.prop));
                    }
                    ("trPr" | "tblPrEx", Some(Frame::Row), Some(t)) => {
                        let props = capture(&mut reader)?;
                        let row = tables.get_mut(t)?.rows.last_mut()?;
                        row.deleted |= props.iter().any(|p| p.prop.0 == "del");
                        row.props.extend(formatting(props));
                    }
                    ("tcPr", Some(Frame::Cell), Some(t)) => {
                        let props = capture(&mut reader)?;
                        let cell = tables.get_mut(t)?.rows.last_mut()?.cells.last_mut()?;
                        cell.deleted |= props.iter().any(|p| p.prop.0 == "cellDel");
                        cell.props.extend(formatting(props));
                    }
                    ("sectPr", _, _) if main => {
                        sections.push(section_refs(capture(&mut reader)?));
                    }
                    ("tbl", _, _) => {
                        open.push(tables.len());
                        tables.push(Table::default());
                        stack.push(Frame::Table);
                    }
                    ("tr", _, Some(t)) => {
                        tables.get_mut(t)?.rows.push(Row::default());
                        stack.push(Frame::Row);
                    }
                    ("tc", _, Some(t)) => {
                        tables
                            .get_mut(t)?
                            .rows
                            .last_mut()?
                            .cells
                            .push(Cell::default());
                        stack.push(Frame::Cell);
                    }
                    ("t", _, _) => stack.push(Frame::Text),
                    (name, _, _) if name.ends_with("PrChange") => skip(&mut reader)?,
                    _ if fallback && element.local_name().as_ref() == "Fallback" => {
                        skip(&mut reader)?;
                    }
                    _ => stack.push(Frame::Other),
                }
            }
            Event::Empty(element) => {
                let name = element.local_name();
                if main && word && name.as_ref() == "sectPr" {
                    sections.push(Vec::new());
                }
                // A symbol (`w:sym`) is text the views show, by its character's code.
                let symbol = (word && name.as_ref() == "sym")
                    .then(|| symbol_of(&element))
                    .flatten();
                if let Some(character) = symbol {
                    whole.push(character);
                    add_text(&mut tables, &open, &character.to_string());
                }
            }
            Event::End(_) => match stack.pop() {
                Some(Frame::Table) => {
                    let done = open.pop()?;
                    let table = tables.get_mut(done)?;
                    table.rows.retain(|row| !row.deleted);
                    for row in &mut table.rows {
                        row.cells.retain(|cell| !cell.deleted);
                    }
                }
                Some(_) => {}
                None => return None,
            },
            Event::Text(text) if stack.last() == Some(&Frame::Text) => {
                whole.push_str(&text);
                add_text(&mut tables, &open, &text);
            }
            Event::GeneralRef(entity) if stack.last() == Some(&Frame::Text) => {
                let text = entity_text(&entity);
                whole.push_str(&text);
                add_text(&mut tables, &open, &text);
            }
            Event::Eof => {
                // A table whose every row's deletion is tracked is gone once they are accepted.
                tables.retain(|table| !table.rows.is_empty());
                return stack.is_empty().then_some((tables, sections, whole));
            }
            Event::Text(_)
            | Event::CData(_)
            | Event::Comment(_)
            | Event::Decl(_)
            | Event::PI(_)
            | Event::DocType(_)
            | Event::GeneralRef(_) => {}
        }
    }
}

/// The text an entity reference in a `w:t` stands for: a predefined entity (`&amp;`) or a
/// character reference (`&#8212;`) resolved, anything else as it is written.
fn entity_text(entity: &quick_xml::events::BytesRef<'_>) -> String {
    if let Ok(Some(character)) = entity.resolve_char_ref() {
        return character.to_string();
    }
    quick_xml::escape::resolve_predefined_entity(entity)
        .map_or_else(|| format!("&{};", &**entity), str::to_owned)
}

/// The character of a `w:sym` element, by its `w:char` attribute's hex code.
fn symbol_of(element: &BytesStart<'_>) -> Option<char> {
    let code = element
        .attributes()
        .flatten()
        .find(|attr| attr.key.local_name().as_ref() == "char")?;
    char::from_u32(u32::from_str_radix(&code.value, 16).ok()?)
}

/// Adds `text` to every open table's text, and to its current row's and cell's.
fn add_text(tables: &mut [Table], open: &[usize], text: &str) {
    for &index in open {
        let Some(table) = tables.get_mut(index) else {
            continue;
        };
        table.text.push_str(text);
        if let Some(row) = table.rows.last_mut() {
            row.text.push_str(text);
            if let Some(cell) = row.cells.last_mut() {
                cell.text.push_str(text);
            }
        }
    }
}

/// A property element's child: its local name and canonical form, and its attributes by local
/// name (for a section's references).
struct Captured {
    prop: Prop,
    attrs: Vec<(String, String)>,
}

/// `props` without the tracked marks ([`MARKS`]).
fn formatting(props: Vec<Captured>) -> impl Iterator<Item = Prop> {
    props
        .into_iter()
        .map(|captured| captured.prop)
        .filter(|(name, _)| !MARKS.contains(&name.as_str()))
}

/// A section's references from its `w:sectPr`'s children.
fn section_refs(children: Vec<Captured>) -> SectionRefs {
    children
        .into_iter()
        .filter_map(|child| {
            let footer = match child.prop.0.as_str() {
                "headerReference" => false,
                "footerReference" => true,
                _ => return None,
            };
            let attr = |key: &str| {
                child
                    .attrs
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value.clone())
            };
            let slot = match attr("type").as_deref() {
                Some("first") => HeaderSlot::FirstPage,
                Some("even") => HeaderSlot::EvenPages,
                _ => HeaderSlot::Default,
            };
            Some((footer, slot, attr("id")?))
        })
        .collect()
}

/// Reads the element just started to its end, each child as one canonical string (a change's
/// old properties left out); `None` when the part does not read.
fn capture<R: BufRead>(reader: &mut NsReader<R>) -> Option<Vec<Captured>> {
    let mut buf = Vec::new();
    let mut children = Vec::new();
    let mut depth = 0_usize;
    let mut current: Option<Captured> = None;
    loop {
        buf.clear();
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(element) => {
                depth += 1;
                if noise(element.local_name().as_ref()) {
                    skip(reader)?;
                    depth -= 1;
                    continue;
                }
                open_element(&element, depth, &mut current);
            }
            Event::Empty(element) => {
                if noise(element.local_name().as_ref()) {
                    continue;
                }
                open_element(&element, depth + 1, &mut current);
                close_element(depth + 1, &mut current, &mut children);
            }
            Event::End(_) => {
                if depth == 0 {
                    return Some(children);
                }
                close_element(depth, &mut current, &mut children);
                depth -= 1;
            }
            Event::Text(text) => {
                if let Some(current) = current.as_mut() {
                    current.prop.1.push_str(&text);
                }
            }
            Event::Eof => return None,
            Event::CData(_)
            | Event::Comment(_)
            | Event::Decl(_)
            | Event::PI(_)
            | Event::DocType(_)
            | Event::GeneralRef(_) => {}
        }
    }
}

/// Whether a property element's child named `name` is left out: a tracked change's old
/// properties (`*PrChange`, `w:tblGridChange`, `w:tblPrExChange`), and the style marks Word
/// works out from a row's or a cell's place in the table (`w:cnfStyle`), which change when a row
/// is inserted above.
fn noise(name: &str) -> bool {
    name.ends_with("PrChange") || matches!(name, "tblGridChange" | "tblPrExChange" | "cnfStyle")
}

/// Starts `element` at `depth` below the captured one: a child (depth 1) starts a new canonical
/// string; a deeper one goes on the current child's.
fn open_element(element: &BytesStart<'_>, depth: usize, current: &mut Option<Captured>) {
    let name = element.local_name().as_ref().to_owned();
    let mut attrs: Vec<(String, String)> = element
        .attributes()
        .flatten()
        .map(|attr| {
            let key = attr.key.local_name().as_ref().to_owned();
            (key, attr.value.to_string())
        })
        .filter(|(key, _)| !key.starts_with("rsid") && key != "paraId" && key != "textId")
        .collect();
    attrs.sort();
    let mut form = format!("{name}[");
    for (key, value) in &attrs {
        let _infallible = write!(form, "{key}={value:?};");
    }
    form.push_str("](");
    if depth == 1 {
        *current = Some(Captured {
            prop: (name, form),
            attrs,
        });
    } else if let Some(current) = current.as_mut() {
        current.prop.1.push_str(&form);
    }
}

/// Ends the element at `depth`: a child (depth 1) is done.
fn close_element(depth: usize, current: &mut Option<Captured>, children: &mut Vec<Captured>) {
    if let Some(open) = current.as_mut() {
        open.prop.1.push(')');
    }
    if depth == 1 {
        children.extend(current.take());
    }
}

/// Reads the element just started to its end, keeping nothing.
fn skip<R: BufRead>(reader: &mut NsReader<R>) -> Option<()> {
    let mut buf = Vec::new();
    let mut depth = 0_usize;
    loop {
        buf.clear();
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(_) => depth += 1,
            Event::End(_) if depth == 0 => return Some(()),
            Event::End(_) => depth -= 1,
            Event::Eof => return None,
            Event::Empty(_)
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
