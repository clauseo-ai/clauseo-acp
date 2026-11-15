//! What the marks scan reads of a Word file's package. Every entry that is XML is read for marks:
//! every name ending in `.xml`, and every part `[Content_Types].xml` gives an XML content type
//! under another name, but the content types themselves and the relationships parts. The
//! relationships decide nothing about what is read; they are read, with the content types, only
//! so that a part either names and the zip lacks is noticed: the scan cannot say such a file
//! holds no changes. Both are read as XML (quick-xml), so a value's references are decoded, a
//! prefixed element is read by its local name, and a part name matches without case, as OPC
//! says.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use quick_xml::events::{BytesStart, Event};

use super::{declared_utf8, utf8};

/// The most parts of each kind (XML parts, relationships parts) one file's scan reads: Word
/// writes a few dozen. A file with more may have changes in those it does not read.
pub(super) const MAX_PARTS: usize = 1_024;

/// The content types part's name, matched without case.
const CONTENT_TYPES: &str = "[Content_Types].xml";

/// A relationships part's content type.
const RELS_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";

/// The zip's entries: their names as written, and lowered, for a match without case.
pub(super) struct Entries {
    pub(super) names: Vec<String>,
    lower: HashSet<String>,
}

/// The zip's entries that are files (not folders); `None` when a name does not read or two
/// names are one part name (they match without case), so one would go unread.
pub(super) fn entries<R: Read + Seek>(zip: &zip::ZipArchive<R>) -> Option<Entries> {
    let mut names = Vec::new();
    let mut lower = HashSet::new();
    for name in zip.file_names() {
        let name = name.ok()?;
        if name.ends_with('/') {
            continue;
        }
        if !lower.insert(name.to_lowercase()) {
            return None;
        }
        names.push(name.into_owned());
    }
    Some(Entries { names, lower })
}

impl Entries {
    /// Whether the zip holds the part `name` (a zip entry name), matched without case.
    fn holds(&self, name: &str) -> bool {
        self.lower.contains(&name.to_lowercase())
    }
}

/// Whether `name` is the content types part.
pub(super) fn is_content_types(name: &str) -> bool {
    name.eq_ignore_ascii_case(CONTENT_TYPES)
}

/// `[Content_Types].xml` as read: content types by extension and by part name, both lowered.
#[derive(Debug, Default)]
pub(super) struct ContentTypes {
    defaults: HashMap<String, String>,
    overrides: HashMap<String, String>,
}

impl ContentTypes {
    /// The content type of the part `name`, if the content types give one.
    fn of(&self, name: &str) -> Option<&str> {
        let lower = name.to_lowercase();
        if let Some(kind) = self.overrides.get(&lower) {
            return Some(kind.as_str());
        }
        let file = lower.rsplit('/').next().unwrap_or_default();
        let (_, extension) = file.rsplit_once('.')?;
        self.defaults.get(extension).map(String::as_str)
    }

    /// Whether the part `name` is a relationships part: named `.rels`, or typed so.
    pub(super) fn is_rels(&self, name: &str) -> bool {
        ends_with(name, ".rels") || self.of(name).is_some_and(|kind| kind == RELS_TYPE)
    }

    /// Whether the part `name` is XML: named `.xml`, or given an XML content type.
    pub(super) fn is_xml(&self, name: &str) -> bool {
        ends_with(name, ".xml") || self.of(name).is_some_and(|kind| kind.ends_with("xml"))
    }

    /// Whether every part the content types name by name is in the zip.
    pub(super) fn all_present(&self, entries: &Entries) -> bool {
        self.overrides.keys().all(|name| entries.holds(name))
    }
}

/// Whether `name` ends with `end` (lowercase), without case.
fn ends_with(name: &str, end: &str) -> bool {
    name.len() >= end.len()
        && name
            .get(name.len() - end.len()..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(end))
}

/// The content types in `xml`; `None` when it does not read as the scan models it.
pub(super) fn content_types(xml: &[u8]) -> Option<ContentTypes> {
    let mut types = ContentTypes::default();
    elements(xml, |name, element| {
        match name {
            "Default" => {
                let extension = attribute(element, "Extension").ok()?;
                let kind = attribute(element, "ContentType").ok()?;
                if let (Some(extension), Some(kind)) = (extension, kind) {
                    types
                        .defaults
                        .insert(extension.to_lowercase(), kind.to_lowercase());
                }
            }
            "Override" => {
                let part = attribute(element, "PartName").ok()?;
                let kind = attribute(element, "ContentType").ok()?;
                if let (Some(part), Some(kind)) = (part, kind) {
                    let part = part_name(part.strip_prefix('/').unwrap_or(&part))?;
                    types
                        .overrides
                        .insert(part.to_lowercase(), kind.to_lowercase());
                }
            }
            _ => {}
        }
        Some(())
    })?;
    Some(types)
}

/// The parts inside the package that the relationships part `rels` (its zip name) names in
/// `xml`, resolved against their source's folder; `None` when it does not read as the scan
/// models it or a target is not a part name inside the package.
pub(super) fn targets(rels: &str, xml: &[u8]) -> Option<Vec<String>> {
    // `word/_rels/document.xml.rels` holds `word/document.xml`'s: its targets are in `word/`
    let folder = match rels.to_ascii_lowercase().rfind("_rels/") {
        Some(at) => rels.get(..at)?,
        None => rels.get(..rels.rfind('/').map_or(0, |at| at + 1))?,
    };
    let mut targets = Vec::new();
    elements(xml, |name, element| {
        if name != "Relationship" {
            return Some(());
        }
        let external = attribute(element, "TargetMode")
            .ok()?
            .is_some_and(|mode| mode.eq_ignore_ascii_case("External"));
        if external {
            return Some(());
        }
        let target = attribute(element, "Target").ok()??;
        // a fragment names a place in a part, not a part
        let target = target.split('#').next().unwrap_or_default();
        if target.is_empty() {
            return Some(());
        }
        targets.push(resolve(folder, &part_name(target)?)?);
        Some(())
    })?;
    Some(targets)
}

/// Whether every one of `targets` is in the zip.
pub(super) fn present(targets: &[String], entries: &Entries) -> bool {
    targets.iter().all(|target| entries.holds(target))
}

/// Each element of `xml` by its local name, to `each`; `None` when `xml` does not read as the
/// scan models it (not well-formed, a doctype, not UTF-8) or `each` answers `None`.
fn elements(xml: &[u8], mut each: impl FnMut(&str, &BytesStart<'_>) -> Option<()>) -> Option<()> {
    if !utf8(xml) {
        return None;
    }
    let mut reader = quick_xml::Reader::from_reader(xml);
    loop {
        match reader.read_event().ok()? {
            Event::Start(element) | Event::Empty(element) => {
                each(element.local_name().as_ref(), &element)?;
            }
            Event::Decl(declaration) => {
                if !declared_utf8(&declaration) {
                    return None;
                }
            }
            Event::DocType(_) => return None,
            Event::Eof => return Some(()),
            Event::End(_)
            | Event::Text(_)
            | Event::CData(_)
            | Event::Comment(_)
            | Event::PI(_)
            | Event::GeneralRef(_) => {}
        }
    }
}

/// An attribute that does not read: the part is not one the scan models.
struct Unread;

/// The value of `element`'s attribute whose local name is `name`, its references decoded, if it
/// has one.
fn attribute(element: &BytesStart<'_>, name: &str) -> Result<Option<String>, Unread> {
    for attribute in element.attributes() {
        let Ok(attribute) = attribute else {
            return Err(Unread);
        };
        if attribute.key.local_name().as_ref() == name {
            let Ok(value) = attribute.normalized_value(quick_xml::XmlVersion::Implicit1_0) else {
                return Err(Unread);
            };
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

/// A part name or a target as a zip entry name: its `%XX` escapes decoded; `None` when one is
/// not an escape or the result is not UTF-8.
fn part_name(uri: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(uri.len());
    let mut rest = uri.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(after.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = after.get(2..)?;
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    String::from_utf8(bytes).ok()
}

/// `target` resolved against the folder `folder` (`""` or ending in `/`) as a zip entry name,
/// without a leading `/`; `None` for a target that climbs out of the package or names no file.
fn resolve(folder: &str, target: &str) -> Option<String> {
    let joined = match target.strip_prefix('/') {
        Some(absolute) => absolute.to_owned(),
        None => format!("{folder}{target}"),
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    if segments.is_empty() || joined.ends_with('/') {
        return None;
    }
    Some(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::{content_types, part_name, resolve, targets};

    #[test]
    fn a_target_resolves_inside_the_package_or_not_at_all() {
        assert_eq!(
            resolve("word/", "./a/../b.xml").as_deref(),
            Some("word/b.xml")
        );
        assert_eq!(
            resolve("word/", "/x/main.xml").as_deref(),
            Some("x/main.xml")
        );
        assert_eq!(resolve("word/", "../../x.xml"), None);
        assert_eq!(resolve("word/", "a/"), None);
        assert_eq!(resolve("", "/"), None);
        assert_eq!(
            part_name("media/image%201.png").as_deref(),
            Some("media/image 1.png")
        );
        assert_eq!(part_name("a%2"), None);
    }

    /// Relationships read as XML: references decoded, a prefixed element by its local name, an
    /// external target left out, a fragment cut, targets resolved against the source's folder.
    #[test]
    fn relationships_read_as_xml() {
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            r#"<r:Relationships xmlns:r="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<r:Relationship Id="rId1" Type="http://x/&#104;&#101;&#97;&#100;&#101;&#114;" Target="&#104;&#101;&#97;&#100;&#101;&#114;1.xml"/>"#,
            r#"<Relationship Id="rId2" Type="t" Target='../custom/n.xml#p2'/>"#,
            r#"<Relationship Id="rId3" Type="t" Target="https://a/b" TargetMode="External"/>"#,
            r#"<Relationship Id="rId4" Type="t" Target="/Word/Footer%201.XML"/>"#,
            r#"</r:Relationships>"#,
        );
        assert_eq!(
            targets("word/_rels/document.xml.rels", xml.as_bytes()).unwrap(),
            ["word/header1.xml", "custom/n.xml", "Word/Footer 1.XML"]
        );
        assert_eq!(
            targets(
                "_rels/.rels",
                br#"<Relationships><Relationship Target="x/main.xml"/></Relationships>"#
            )
            .unwrap(),
            ["x/main.xml"]
        );
        // not well-formed, or a target that climbs out of the package
        assert_eq!(
            targets("_rels/.rels", b"<Relationships><Relationship"),
            None
        );
        assert_eq!(
            targets(
                "_rels/.rels",
                br#"<Relationships><Relationship Target="../x.xml"/></Relationships>"#
            ),
            None
        );
    }

    #[test]
    fn content_types_say_which_parts_are_xml_and_which_are_relationships() {
        let xml = concat!(
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
            r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
            r#"<Default Extension="XML" ContentType="application/xml"/>"#,
            r#"<Default Extension="bin" ContentType="application/octet-stream"/>"#,
            r#"<Override PartName="/custom/head.bin" "#,
            r#"ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>"#,
            r#"</Types>"#,
        );
        let types = content_types(xml.as_bytes()).unwrap();
        assert!(types.is_xml("custom/head.bin"));
        assert!(types.is_xml("Custom/Head.BIN"));
        assert!(!types.is_xml("word/media/image1.bin"));
        assert!(types.is_xml("word/letterhead.XML"));
        assert!(types.is_rels("word/_rels/document.xml.rels"));
        assert!(!types.is_rels("word/document.xml"));
        assert!(content_types(b"<Types><Default").is_none());
    }
}
