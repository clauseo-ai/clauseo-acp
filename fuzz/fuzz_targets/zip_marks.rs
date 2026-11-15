//! revisions: the tracked-changes scan `zip_has_marks` (clauseo-host, revisions/marks.rs) against
//! an oracle that shares none of its XML reading.
//!
//! Items driven: `zip_has_marks`, on a zip in memory. build.rs compiles marks.rs (and its parts.rs)
//! into this target, so the scan runs as the host builds it without this crate depending on
//! clauseo-host. That makes it the one target that reaches a private item; a marks.rs that needs
//! more of the host than `super::is_docx` and `crate::write::open_read_nofollow` needs a stand-in
//! below.
//!
//! Input: byte 0 = mode, rest = payload.
//!   mode & 1 == 0: the payload is choices for docx_gen.rs, which builds a small .docx from them.
//!   mode & 1 == 1: the payload is the zip, as it is (the seeds hold the repo's marks fixtures).
//!
//! The oracle opens the zip with the zip crate and parses every entry that is XML with roxmltree:
//! named `.xml` or `.rels` (any folder, any case), or given a content type ending in `xml` that
//! is not an image's by `[Content_Types].xml` (read with roxmltree too, parameters dropped).
//! roxmltree resolves namespaces itself, character references and internal entities included.
//! It answers "marks" when any element in Word's Transitional or Strict main namespace has a
//! mark's local name (docx_gen.rs `MARKS`, marks.rs's list) in a part but the content types,
//! "unreadable" when the zip opens but an XML entry does not read whole through it (or is over
//! the scan's 16 MiB a part), "unparsed" when an entry is not UTF-8 or roxmltree refuses it,
//! "types marks" when the only mark is in `[Content_Types].xml`, "no zip" when the zip does not
//! open, and "clean" otherwise.
//!
//! Invariants (what #1068 promises: "no changes" only for a zip read whole, both namespaces):
//!   - oracle "marks": `zip_has_marks` is not `Some(false)`.
//!   - oracle "unreadable": `zip_has_marks` is not `Some(false)` (it reads with the same crate).
//!   - oracle "no zip": `zip_has_marks` is not `Some(false)`.
//!   - a generated file whose package the design always offers (docx_gen.rs `must_offer`: a
//!     named part missing, an aFChunk, two entries of one name, no content types) is not
//!     `Some(false)`.
//!   - a generated file with an odd SVG (typed `image/svg+xml`) answers as its twin with a plain
//!     SVG does: an image never decides the answer.
//!
//! Counted, never asserted (printed every 2^15 inputs): "clean" answered `Some(true)` (offered
//! without a change, allowed; most are `must_offer` files), `None` (a read error, which a zip
//! in memory should not have), and a mark only in `[Content_Types].xml` answered `Some(false)`
//! (the design reads that part for content types, never for marks; Word never shows it).
#![no_main]
#![expect(
    clippy::disallowed_methods,
    reason = "fuzz harness, not app code: fuzz_target! writes its debug file with std::fs"
)]

use std::io::Read as _;
use std::sync::atomic::{AtomicU64, Ordering};

use libfuzzer_sys::fuzz_target;

mod docx_gen;

/// marks.rs's `super::is_docx`.
fn is_docx(path: &clauseo_protocol::MatterPath) -> bool {
    std::path::Path::new(path.as_str())
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
}

/// marks.rs's `crate::write`: only `check` opens a file, and this target never calls it.
mod write {
    pub(crate) fn open_read_nofollow(path: &std::path::Path) -> std::io::Result<std::fs::File> {
        std::fs::File::open(path)
    }
}

#[expect(
    dead_code,
    reason = "the copied scan keeps the host's helpers this target never calls"
)]
mod marks {
    include!(concat!(env!("OUT_DIR"), "/marks.rs"));
}

/// The scan's cap on one part, decompressed (marks.rs `MAX_XML_BYTES`).
const PART_CAP: u64 = 16 * 1024 * 1024;

/// What the oracle found.
#[derive(Debug)]
enum Verdict {
    /// A mark, in this entry.
    Marks(String),
    /// This entry did not read whole through the zip crate.
    Unreadable(String),
    /// An entry is not UTF-8, or roxmltree refused it.
    Unparsed,
    /// A mark only in `[Content_Types].xml`, which the design reads for types, never for marks.
    TypesMarks,
    /// The zip does not open.
    NoZip,
    Clean,
}

fn ends_with(name: &str, end: &[u8]) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= end.len() && bytes[bytes.len() - end.len()..].eq_ignore_ascii_case(end)
}

/// `[Content_Types].xml`: lowered media types by lowered extension and by lowered part name.
#[derive(Default)]
struct Types {
    defaults: Vec<(String, String)>,
    overrides: Vec<(String, String)>,
}

impl Types {
    fn parse(text: &str) -> Option<Types> {
        let doc = roxmltree::Document::parse(text).ok()?;
        let media = |kind: &str| kind.split(';').next().unwrap_or("").trim().to_lowercase();
        let mut types = Types::default();
        for node in doc.descendants().filter(|node| node.is_element()) {
            let kind = node.attribute("ContentType").map(media);
            match (node.tag_name().name(), kind) {
                ("Default", Some(kind)) => {
                    if let Some(extension) = node.attribute("Extension") {
                        types.defaults.push((extension.to_lowercase(), kind));
                    }
                }
                ("Override", Some(kind)) => {
                    if let Some(part) = node.attribute("PartName") {
                        let part = part.strip_prefix('/').unwrap_or(part).to_lowercase();
                        types.overrides.push((part, kind));
                    }
                }
                _ => {}
            }
        }
        Some(types)
    }

    fn is_xml(&self, name: &str) -> bool {
        if ends_with(name, b".xml") || ends_with(name, b".rels") {
            return true;
        }
        let lower = name.to_lowercase();
        let kind = self
            .overrides
            .iter()
            .find(|(part, _)| *part == lower)
            .or_else(|| {
                let extension = lower.rsplit('/').next()?.rsplit_once('.')?.1;
                self.defaults.iter().find(|(ext, _)| ext == extension)
            });
        kind.is_some_and(|(_, kind)| kind.ends_with("xml") && !kind.starts_with("image/"))
    }
}

/// Whether `text`, one XML part, holds an element in either Word namespace with a mark's name;
/// `None` when roxmltree refuses it.
fn part_has_marks(text: &str) -> Option<bool> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, options).ok()?;
    Some(
        doc.descendants()
            .filter(|node| node.is_element())
            .any(|node| {
                let tag = node.tag_name();
                matches!(tag.namespace(), Some(docx_gen::T_NS | docx_gen::S_NS))
                    && docx_gen::MARKS.contains(&tag.name())
            }),
    )
}

fn oracle(zip: &[u8]) -> Verdict {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(zip)) else {
        return Verdict::NoZip;
    };
    let mut unreadable = None;
    let mut unparsed = false;
    let mut types_marks = false;
    let mut types = Types::default();
    let types_name = archive
        .file_names()
        .filter_map(Result::ok)
        .find(|name| name.eq_ignore_ascii_case("[Content_Types].xml"))
        .map(std::borrow::Cow::into_owned);
    if let Some(name) = types_name {
        let mut bytes = Vec::new();
        let read = archive
            .by_name(&name)
            .ok()
            .and_then(|entry| entry.take(PART_CAP).read_to_end(&mut bytes).ok());
        match read
            .and_then(|_| std::str::from_utf8(&bytes).ok())
            .and_then(Types::parse)
        {
            Some(read) => types = read,
            None => unparsed = true,
        }
    }
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            // its name can't be read either: an entry that may be XML, unread
            unreadable.get_or_insert_with(|| format!("#{index}"));
            continue;
        };
        let Ok(name) = entry.name().map(std::borrow::Cow::into_owned) else {
            unreadable.get_or_insert_with(|| format!("#{index}"));
            continue;
        };
        if entry.is_dir() || !types.is_xml(&name) {
            continue;
        }
        let mut bytes = Vec::new();
        let read = (&mut entry).take(PART_CAP + 1).read_to_end(&mut bytes);
        if read.is_err() || bytes.len() as u64 > PART_CAP {
            unreadable.get_or_insert(name);
            continue;
        }
        let text = std::str::from_utf8(&bytes).ok();
        match text.and_then(|text| part_has_marks(text.strip_prefix('\u{feff}').unwrap_or(text))) {
            Some(true) if name.eq_ignore_ascii_case("[Content_Types].xml") => types_marks = true,
            Some(true) => return Verdict::Marks(name),
            Some(false) => {}
            None => unparsed = true,
        }
    }
    match (unreadable, unparsed, types_marks) {
        (Some(name), _, _) => Verdict::Unreadable(name),
        (None, true, _) => Verdict::Unparsed,
        (None, false, true) => Verdict::TypesMarks,
        (None, false, false) => Verdict::Clean,
    }
}

static RUNS: AtomicU64 = AtomicU64::new(0);
static OVER_OFFERED: AtomicU64 = AtomicU64::new(0);
static CLEAN: AtomicU64 = AtomicU64::new(0);
static NONE: AtomicU64 = AtomicU64::new(0);
static TYPES_MARKS_FALSE: AtomicU64 = AtomicU64::new(0);

fn count(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, payload)) = data.split_first() else {
        return;
    };
    let generated = if mode & 1 == 0 {
        match docx_gen::build(payload) {
            Some(docx) => Some(docx),
            None => return,
        }
    } else {
        None
    };
    let zip = generated
        .as_ref()
        .map_or(payload, |docx| docx.zip.as_slice());
    let spec = generated
        .as_ref()
        .map_or("raw zip", |docx| docx.spec.as_str());

    let answer = marks::fuzz_zip_has_marks(zip);
    let verdict = oracle(zip);
    match (&verdict, answer) {
        (Verdict::Marks(part), Some(false)) => {
            panic!("oracle found a mark in {part:?}, zip_has_marks said Some(false): {spec}")
        }
        (Verdict::Unreadable(part), Some(false)) => {
            panic!("{part:?} does not read whole, zip_has_marks said Some(false): {spec}")
        }
        (Verdict::NoZip, Some(false)) => {
            panic!("the zip does not open, zip_has_marks said Some(false): {spec}")
        }
        (Verdict::Clean, Some(true)) => count(&OVER_OFFERED),
        // counted, not asserted: Word never renders [Content_Types].xml
        (Verdict::TypesMarks, Some(false)) => count(&TYPES_MARKS_FALSE),
        _ => {}
    }
    if matches!(verdict, Verdict::Clean) {
        count(&CLEAN);
    }
    if answer.is_none() {
        count(&NONE);
    }
    if let Some(docx) = &generated {
        if let (Some(why), Some(false)) = (docx.must_offer, answer) {
            panic!("{why}, zip_has_marks said Some(false): {spec}");
        }
        if let Some(twin) = &docx.svg_twin {
            let plain = marks::fuzz_zip_has_marks(twin);
            assert_eq!(answer, plain, "the odd SVG changed the answer: {spec}");
        }
    }
    let runs = RUNS.fetch_add(1, Ordering::Relaxed) + 1;
    if runs.is_multiple_of(1 << 15) {
        eprintln!(
            "zip_marks: {runs} runs, {} clean, {} clean but Some(true), {} None, \
             {} a mark only in the content types but Some(false)",
            CLEAN.load(Ordering::Relaxed),
            OVER_OFFERED.load(Ordering::Relaxed),
            NONE.load(Ordering::Relaxed),
            TYPES_MARKS_FALSE.load(Ordering::Relaxed),
        );
    }
});
