//! A Hindi Word document through an edit, a checkpoint and a restore (audit 3d). The matter
//! holds `patra.docx` (the letter in Devanagari, NFC, with a nukta paragraph). The edit is
//! `patra-edited.docx` (the same letter with one paragraph added), written over the file the
//! way a Word save would; a prompt then takes a before-turn checkpoint of the folder as it now
//! is. Restoring the first checkpoint must give back the original's bytes exactly, and its
//! `word/document.xml` text (the `w:t` runs, joined) must be the original's and still NFC;
//! restoring the edit's checkpoint must give the edit back. Once with a Devanagari file name
//! (`पत्र.docx`, NFC) as well.
//!
//! The fixtures come from `tests/fixtures/hindi/generate.py`. The zip and XML reads below are
//! the test's own (`flate2` and `quick-xml`, pinned in the workspace), not an archive crate.

use std::io::Read as _;

use clauseo_protocol::{
    CheckpointId, CheckpointKind, CommandOutcome, HostCommand, MatterId, MatterPath,
    RestoreRequest, StopReason, StreamId,
};
use quick_xml::XmlVersion;
use quick_xml::events::{BytesRef, Event};

use crate::support::{Dir, FAKE, Running, Scenario};
use crate::versions::matter_events;
use crate::write;

/// `patra.docx`: the letter as `generate.py` writes it.
const ORIGINAL: &[u8] = include_bytes!("../fixtures/hindi/patra.docx");

/// `patra-edited.docx`: the letter with one paragraph added, the edit's stand-in.
const EDITED: &[u8] = include_bytes!("../fixtures/hindi/patra-edited.docx");

/// `पत्र.docx` with the name in NFC: प, त, ् (virama), र.
const DEVANAGARI_NAME: &str = "\u{092A}\u{0924}\u{094D}\u{0930}.docx";

#[test]
fn a_hindi_word_document_round_trips_through_an_edit_a_checkpoint_and_a_restore() {
    round_trip("hindi-word-round-trip", "patra.docx");
}

#[test]
fn a_hindi_word_document_with_a_devanagari_name_round_trips_too() {
    round_trip("hindi-word-devanagari-name", DEVANAGARI_NAME);
}

fn round_trip(test: &str, name: &str) {
    assert!(
        unicode_normalization::is_nfc(name),
        "{name} is not NFC, so it is not the name under test"
    );
    let dir = Dir::new(test, &[(FAKE, Scenario::Bundled("prompt-stream"))]);
    let host = dir.start(0);
    let folder = dir.work_path("letter");
    write::write(&folder.join(name), ORIGINAL);
    let matter = host.create_matter("letter", &folder);
    let thread = host.create_thread(matter, FAKE);
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    let original_text = word_text(ORIGINAL);
    assert!(
        original_text.contains("३,४७,२१५"),
        "the fixture's text: {original_text}"
    );

    // The edit: the file is saved with the change, then a prompt. Its before-turn checkpoint
    // holds the folder as it is now (only a restore or an undo writes an outside-edits one).
    write::write(&folder.join(name), EDITED);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Next.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn);
    host.settle();
    let edited = matter_events(&host, matter)
        .checkpoint(CheckpointKind::BeforeTurn)
        .unwrap_or_else(|| panic!("no checkpoint of the edit: {}", events.dump()));

    // The first checkpoint: the original's bytes, exactly, and its text, NFC.
    restore(&host, matter, import, name);
    let read = || std::fs::read(folder.join(name)).unwrap();
    assert_eq!(
        read(),
        ORIGINAL,
        "the restored bytes differ from the original"
    );
    let restored = word_text(&read());
    assert_eq!(restored, original_text);
    assert!(
        unicode_normalization::is_nfc(&restored),
        "the restored text is not NFC"
    );

    // The second checkpoint: the edit is back.
    restore(&host, matter, edited, name);
    assert_eq!(read(), EDITED, "the restored bytes differ from the edit");
    assert_eq!(word_text(&read()), word_text(EDITED));
    assert_ne!(word_text(EDITED), original_text, "the edit changed no text");
    host.shutdown();
}

/// `RestoreVersion` of `checkpoint` for the one file `name`. The outcome names the checkpoint
/// the restore wrote (kind `Restore`), which is new and not the one restored from.
fn restore(host: &Running, matter: MatterId, checkpoint: CheckpointId, name: &str) {
    let outcome = host.ok(HostCommand::RestoreVersion {
        matter,
        checkpoint,
        scope: RestoreRequest::Files {
            paths: vec![MatterPath::new(name).unwrap()],
        },
    });
    let CommandOutcome::VersionRestored { checkpoint: wrote } = outcome else {
        panic!("RestoreVersion answered {outcome:?}");
    };
    assert_ne!(
        wrote, checkpoint,
        "the restore wrote no checkpoint of its own"
    );
}

/// The text of a `.docx`'s `word/document.xml`: every `w:t` run's text, in order, joined.
fn word_text(docx: &[u8]) -> String {
    let xml = String::from_utf8(entry(docx, "word/document.xml")).unwrap();
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut text = String::new();
    let mut in_run_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(start) if start.name().as_ref() == "w:t" => in_run_text = true,
            Event::End(end) if end.name().as_ref() == "w:t" => in_run_text = false,
            Event::Text(raw) if in_run_text => {
                text.push_str(&raw.xml_content(XmlVersion::Explicit1_0));
            }
            Event::GeneralRef(entity) if in_run_text => text.push_str(&entity_text(&entity)),
            Event::Eof => break,
            _ => {}
        }
    }
    text
}

/// What a general reference (`&amp;`, `&#x935;`) stands for; the entities a document of the
/// fixtures could hold, and no other.
fn entity_text(entity: &BytesRef<'_>) -> String {
    if let Some(c) = entity.resolve_char_ref().unwrap() {
        return c.to_string();
    }
    let name = entity.xml10_content();
    match &*name {
        "amp" => "&".to_owned(),
        "lt" => "<".to_owned(),
        "gt" => ">".to_owned(),
        "quot" => "\"".to_owned(),
        "apos" => "'".to_owned(),
        other => panic!("an entity this test does not know: &{other};"),
    }
}

/// The bytes of the zip entry `name` in `zip`, found through the central directory; stored or
/// deflated entries only, which is all `zipfile` writes for a `.docx`.
fn entry(zip: &[u8], name: &str) -> Vec<u8> {
    let u16_at = |at: usize| usize::from(u16::from_le_bytes([zip[at], zip[at + 1]]));
    let u32_at = |at: usize| {
        let bytes: [u8; 4] = zip[at..at + 4].try_into().unwrap();
        usize::try_from(u32::from_le_bytes(bytes)).unwrap()
    };
    let end = (0..zip.len().saturating_sub(21))
        .rev()
        .find(|&at| zip[at..at + 4] == [0x50, 0x4b, 0x05, 0x06])
        .expect("no end of central directory");
    let mut at = u32_at(end + 16);
    for _ in 0..u16_at(end + 10) {
        assert_eq!(
            zip[at..at + 4],
            [0x50, 0x4b, 0x01, 0x02],
            "a central directory entry"
        );
        let (method, packed) = (u16_at(at + 10), u32_at(at + 20));
        let (names, extra, comment) = (u16_at(at + 28), u16_at(at + 30), u16_at(at + 32));
        if &zip[at + 46..at + 46 + names] == name.as_bytes() {
            let local = u32_at(at + 42);
            let data = local + 30 + u16_at(local + 26) + u16_at(local + 28);
            let packed = &zip[data..data + packed];
            return match method {
                0 => packed.to_vec(),
                8 => {
                    let mut out = Vec::new();
                    flate2::read::DeflateDecoder::new(packed)
                        .read_to_end(&mut out)
                        .unwrap();
                    out
                }
                other => panic!("zip method {other} for {name}"),
            };
        }
        at += 46 + names + extra + comment;
    }
    panic!("no {name} in the zip")
}
