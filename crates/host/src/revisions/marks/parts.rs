//! Which parts of a Word file the marks scan reads: the parts stetdoc reads tracked changes from,
//! found the way stetdoc finds them. The main document is the package's `officeDocument`
//! relationship (`_rels/.rels`; `word/document.xml` when it names none), and its own
//! relationships (`word/_rels/document.xml.rels`) name its headers, footers, footnotes, endnotes
//! and comments. A relationship to outside the package (`TargetMode="External"`) is not a part.
//!
//! The relationship parts are matched as bytes, never parsed as XML, like the rest of the scan:
//! each `<Relationship` element's `Type`, `Target` and `TargetMode` attributes are read, with the
//! XML entities in a value decoded, and a target is resolved against its source part's folder.

/// Where the main document is when the package names none.
const MAIN_FALLBACK: &str = "word/document.xml";

/// The end of the relationship type that names the main document.
const MAIN_TYPE: &str = "/officeDocument";

/// The ends of the relationship types of the parts besides the main document that stetdoc reads
/// tracked changes from (0.9.3 calls them the `headers`, `notes` and `comments` stories).
const STORY_TYPES: &[&str] = &["/header", "/footer", "/footnotes", "/endnotes", "/comments"];

/// The most parts one file's scan reads: Word writes at most six headers and footers a section.
pub(super) const MAX_PARTS: usize = 1_024;

/// One `<Relationship>`: its type, its target and whether that target is outside the package.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Relationship {
    pub(super) kind: String,
    pub(super) target: String,
    pub(super) external: bool,
}

/// The main document's name, from the package's relationships `rels` (empty: no such part).
pub(super) fn main_part(rels: &[u8]) -> String {
    relationships(rels)
        .find(|rel| !rel.external && rel.kind.ends_with(MAIN_TYPE))
        .and_then(|rel| resolve("", &rel.target))
        .unwrap_or_else(|| MAIN_FALLBACK.to_owned())
}

/// The name of the relationships part of the part `name`: `word/document.xml`'s is
/// `word/_rels/document.xml.rels`.
pub(super) fn rels_of(name: &str) -> String {
    let (folder, file) = split(name);
    format!("{folder}_rels/{file}.rels")
}

/// The story parts the main document `main`'s relationships `rels` name, in their order, each
/// once, at most [`MAX_PARTS`], `main` itself not among them.
pub(super) fn story_parts(main: &str, rels: &[u8]) -> Vec<String> {
    let (folder, _) = split(main);
    let mut parts: Vec<String> = Vec::new();
    for rel in relationships(rels) {
        if rel.external || !STORY_TYPES.iter().any(|end| rel.kind.ends_with(end)) {
            continue;
        }
        let Some(name) = resolve(folder, &rel.target) else {
            continue;
        };
        if name != main && !parts.contains(&name) {
            parts.push(name);
        }
        if parts.len() >= MAX_PARTS {
            break;
        }
    }
    parts
}

/// A part name's folder, with its trailing `/` (`""` at the top), and its file name.
fn split(name: &str) -> (&str, &str) {
    match name.rfind('/') {
        Some(at) => name.split_at(at + 1),
        None => ("", name),
    }
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

/// Every `<Relationship` element of `xml` as a [`Relationship`]; one without a `Type` or a
/// `Target` is left out.
pub(super) fn relationships(xml: &[u8]) -> impl Iterator<Item = Relationship> + '_ {
    const OPEN: &[u8] = b"<Relationship";
    memchr::memmem::find_iter(xml, OPEN).filter_map(|at| {
        let rest = xml.get(at + OPEN.len()..)?;
        // `<Relationships`, the root, is another element
        if !rest.first().is_some_and(u8::is_ascii_whitespace) {
            return None;
        }
        let end = memchr::memchr(b'>', rest)?;
        let mut kind = None;
        let mut target = None;
        let mut external = false;
        for (name, value) in attributes(rest.get(..end)?) {
            match name {
                b"Type" => kind = Some(text(value)),
                b"Target" => target = Some(text(value)),
                b"TargetMode" => external = value.eq_ignore_ascii_case(b"External"),
                _ => {}
            }
        }
        Some(Relationship {
            kind: kind?,
            target: target?,
            external,
        })
    })
}

/// The `name="value"` (or `name='value'`) pairs of an element's tag after its name, until one
/// does not read.
fn attributes(mut tag: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut pairs = Vec::new();
    loop {
        let Some(eq) = memchr::memchr(b'=', tag) else {
            return pairs;
        };
        let name = tag.get(..eq).unwrap_or_default().trim_ascii();
        let value = tag.get(eq + 1..).unwrap_or_default().trim_ascii_start();
        let Some((&quote, value)) = value.split_first() else {
            return pairs;
        };
        if quote != b'"' && quote != b'\'' {
            return pairs;
        }
        let Some(close) = memchr::memchr(quote, value) else {
            return pairs;
        };
        pairs.push((name, value.get(..close).unwrap_or_default()));
        tag = value.get(close + 1..).unwrap_or_default();
    }
}

/// An attribute's value as text, its five predefined XML entities decoded.
fn text(value: &[u8]) -> String {
    String::from_utf8_lossy(value)
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::{Relationship, main_part, relationships, rels_of, resolve, story_parts};

    const DOC_RELS: &str = concat!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://x/rel">"#,
        r#"<Relationship Id="rId1" Type="http://x/officeDocument/2006/relationships/styles" "#,
        r#"Target="styles.xml"/>"#,
        r#"<Relationship Target='header1.xml' Id="rId2" "#,
        "\n  Type=\"http://x/officeDocument/2006/relationships/header\"/>",
        r#"<Relationship Id="rId3" Type="http://x/relationships/footer" Target="/word/f&amp;1.xml"/>"#,
        r#"<Relationship Id="rId4" Type="http://x/relationships/footnotes" Target="notes/fn.xml"/>"#,
        r#"<Relationship Id="rId5" Type="http://x/relationships/endnotes" Target="../en.xml"/>"#,
        r#"<Relationship Id="rId6" Type="http://x/relationships/comments" Target="comments.xml"/>"#,
        r#"<Relationship Id="rId7" Type="http://x/relationships/header" Target="header1.xml"/>"#,
        r#"<Relationship Id="rId8" Type="http://x/relationships/header" Target="http://a/h.xml" "#,
        r#"TargetMode="External"/>"#,
        r#"<Relationship Id="rId9" Type="http://x/relationships/footer" Target="../../out.xml"/>"#,
        r#"</Relationships>"#,
    );

    #[test]
    fn the_story_parts_are_the_ones_the_document_s_relationships_name() {
        assert_eq!(
            story_parts("word/document.xml", DOC_RELS.as_bytes()),
            [
                "word/header1.xml",
                "word/f&1.xml",
                "word/notes/fn.xml",
                "en.xml",
                "word/comments.xml",
            ]
        );
        assert_eq!(rels_of("word/document.xml"), "word/_rels/document.xml.rels");
        assert_eq!(rels_of("doc.xml"), "_rels/doc.xml.rels");
    }

    #[test]
    fn the_main_document_is_the_package_s_office_document() {
        let rels = concat!(
            r#"<Relationships><Relationship Id="rId2" Type="http://x/relationships/core-properties" "#,
            r#"Target="docProps/core.xml"/><Relationship Id="rId1" "#,
            r#"Type="http://x/officeDocument/2006/relationships/officeDocument" "#,
            r#"Target="/word/document2.xml"/></Relationships>"#,
        );
        assert_eq!(main_part(rels.as_bytes()), "word/document2.xml");
        assert_eq!(main_part(b"<Relationships/>"), "word/document.xml");
        assert_eq!(main_part(b""), "word/document.xml");
    }

    #[test]
    fn a_relationship_reads_in_either_quote_and_a_broken_one_not_at_all() {
        let xml = br#"<Relationship Type='t' Target="a.xml" TargetMode="external"/><Relationship Type="t"/><Relationship Type="t" Target=broken/>"#;
        assert_eq!(
            relationships(xml).collect::<Vec<_>>(),
            [Relationship {
                kind: "t".to_owned(),
                target: "a.xml".to_owned(),
                external: true,
            }]
        );
        assert_eq!(
            resolve("word/", "./a/../b.xml").as_deref(),
            Some("word/b.xml")
        );
        assert_eq!(resolve("word/", "../../x.xml"), None);
        assert_eq!(resolve("word/", "a/"), None);
        assert_eq!(resolve("", "/"), None);
    }
}
