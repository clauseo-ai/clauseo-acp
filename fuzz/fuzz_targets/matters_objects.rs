//! matters_objects: the loose-object integrity paths of clauseo-matters, through its public API.
//!
//! Items driven: `Matter::create`, `before_turn`, `after_turn` and `undo`. A checkpoint writes
//! brief.txt as a loose blob object. The input then replaces that object file. The paths hit are:
//! the write-side object check (history/durable.rs `present`, with history/check.rs `whole` and
//! `header`); the blob read for a file over 8 MiB (history/blob.rs `Blobs::read`, streamed and
//! checked); and the undo read-back (`Matter::undo`, which reads the blob through
//! history/gix_store.rs and the hash-checked history/objects.rs `Objects::read_blob`).
//!
//! Each iteration makes a fresh matter in a scratch folder under the temp dir (see scratch.rs).
//! It writes brief.txt and takes a checkpoint, so the blob's loose object exists. It then
//! overwrites that object file with bytes made from the input, and then either:
//!   (a) if sel & 4: rewrites brief.txt with the same bytes (newer mtime) and checkpoints again,
//!       which must find the object present and check it; and
//!   (b) edits brief.txt, checkpoints, and undoes that step, which reads the blob back.
//!
//! Input: byte 0 = selector, rest = payload.
//!   sel & 3: 0 the object file is the payload; 1 a zlib stream (stored blocks) of the payload;
//!            2 zlib of "blob <payload len>\0" + payload; 3 zlib of "blob <content len>\0" +
//!            the payload cycled to the content's length (a well-formed object, wrong bytes).
//!   sel & 4: run (a) first.
//!   sel & 0xf8 == 0xa0: the content is 8 MiB + 4 KiB (the streamed path) instead of 74 bytes.
//!            That is about 1 s per exec under ASan, so it is 1 in 32 selectors.
//!
//! Invariants: no panic or hang. brief.txt afterwards is either the edited bytes or the original
//! bytes. An undo that reports brief.txt restored leaves exactly the original bytes in it. After
//! (a) has checked and rewritten the object, the undo succeeds and restores brief.txt.
#![no_main]

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use clauseo_matters::{
    Limits, Marker, Matter, NewMatter, OsProbe, Retention, Settings, TurnMeta, UndoRequest,
};
use clauseo_protocol::{
    CheckpointId, FileChoice, FileDecision, MatterId, MatterPath, ThreadId, Timestamp, TurnId,
};
use libfuzzer_sys::fuzz_target;

mod scratch;

const SMALL: &[u8] =
    b"Heads of terms\nThe Seller shall deliver the Goods on the Completion Date.\n";
const EDITED: &[u8] = b"Heads of terms (edited by the agent)\n";

fn big() -> &'static [u8] {
    static CELL: OnceLock<Vec<u8>> = OnceLock::new();
    CELL.get_or_init(|| {
        let line = b"Clause 4.1. The Seller shall deliver the Goods on the Completion Date. ";
        line.iter()
            .copied()
            .cycle()
            .take(8 * 1024 * 1024 + 4096)
            .collect()
    })
}

fn id<T: std::str::FromStr>(n: u64) -> T
where
    T::Err: std::fmt::Debug,
{
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

fn settings(root: &Path) -> Settings {
    Settings {
        matters_root: root.join("homes"),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
    }
}

fn meta(n: u64) -> TurnMeta {
    TurnMeta {
        checkpoint: id::<CheckpointId>(n),
        thread: id::<ThreadId>(100),
        turn: id::<TurnId>(200 + n),
        at: Timestamp::new(1_760_000_000_000 + n as i64 * 1000).unwrap(),
    }
}

fn blob_hex(content: &[u8]) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(format!("blob {}\0", content.len()).as_bytes());
    h.update(content);
    h.digest().to_string()
}

/// A zlib stream of `data` in stored (uncompressed) deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = data.chunks(65535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

fn corrupt_bytes(form: u8, payload: &[u8], content: &[u8]) -> Vec<u8> {
    match form {
        0 => payload.to_vec(),
        1 => zlib_stored(payload),
        2 => {
            let mut body = format!("blob {}\0", payload.len()).into_bytes();
            body.extend_from_slice(payload);
            zlib_stored(&body)
        }
        _ => {
            let mut body = format!("blob {}\0", content.len()).into_bytes();
            if payload.is_empty() {
                body.resize(body.len() + content.len(), 0);
            } else {
                body.extend(payload.iter().copied().cycle().take(content.len()));
            }
            zlib_stored(&body)
        }
    }
}

fn write_with_mtime(path: &Path, bytes: &[u8], mtime: SystemTime) {
    std::fs::write(path, bytes).unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(mtime))
        .unwrap();
}

fn replace_object(path: &Path, bytes: &[u8]) {
    // Loose objects are read-only; replace the file.
    let _ = std::fs::remove_file(path);
    std::fs::write(path, bytes).unwrap();
}

fuzz_target!(|data: &[u8]| {
    let Some((&sel, payload)) = data.split_first() else {
        return;
    };
    let content: &[u8] = if sel & 0xf8 == 0xa0 { big() } else { SMALL };
    let form = sel & 3;
    let recheck = sel & 4 != 0;

    let root = scratch::create("matters-objects");
    std::fs::create_dir_all(root.join("homes")).unwrap();
    let folder = root.join("matter");
    let matter_id: MatterId = id(1);
    let mut matter = Matter::create(
        settings(&root),
        NewMatter {
            id: matter_id,
            folder: folder.clone(),
            created_at: Timestamp::new(1_760_000_000_000).unwrap(),
            marker: Marker::None,
            checkpoint: id(2),
        },
    )
    .expect("matter create");
    let brief = folder.join("brief.txt");
    let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_760_000_000);
    write_with_mtime(&brief, content, t0);
    let first = matter.before_turn(&meta(3)).expect("first checkpoint");
    let hex = blob_hex(content);
    let object = root
        .join("homes")
        .join(matter_id.to_string())
        .join("history.git/objects")
        .join(&hex[..2])
        .join(&hex[2..]);
    std::fs::metadata(&object)
        .unwrap_or_else(|e| panic!("no loose object at {}: {e}", object.display()));
    let corrupt = corrupt_bytes(form, payload, content);
    replace_object(&object, &corrupt);

    let mut before = first.outcome.id();
    let mut rechecked_and_rewritten = false;
    if recheck {
        write_with_mtime(&brief, content, t0 + Duration::from_secs(10));
        match matter.before_turn(&meta(4)) {
            Ok(checkpoint) => {
                before = checkpoint.outcome.id();
                let now = std::fs::read(&object).ok();
                rechecked_and_rewritten = now.as_deref().is_some_and(|b| b != corrupt.as_slice());
            }
            Err(e) => panic!("re-checkpointing unchanged content over a bad object failed: {e:?}"),
        }
    }

    write_with_mtime(&brief, EDITED, t0 + Duration::from_secs(20));
    let after = matter
        .after_turn(&meta(5), &BTreeSet::new())
        .expect("edit checkpoint");
    let path = MatterPath::new("brief.txt").unwrap();
    let decisions = [FileDecision {
        path: path.clone(),
        choice: FileChoice::Undo,
    }];
    let undone = matter.undo(&UndoRequest {
        before,
        after: after.outcome.id(),
        decisions: &decisions,
        thread: Some(id(100)),
        turn: Some(id(205)),
        outside_edits: id(6),
        checkpoint: id(7),
        at: Timestamp::new(1_760_000_030_000).unwrap(),
    });
    let now = std::fs::read(&brief).unwrap_or_default();
    // A corrupt object, in any form, must make the undo fail or restore the original bytes,
    // never other bytes: objects.rs checks every blob it hands out against its id.
    assert!(
        now == EDITED || now == content,
        "brief.txt is {} bytes that are neither the edit nor the original (object form {form}, \
         re-checked {recheck}, undo ok {})",
        now.len(),
        undone.is_ok()
    );
    if let Ok(u) = &undone
        && u.restored.contains(&path)
    {
        assert!(
            now == content,
            "undo reported brief.txt restored but left {} bytes that differ from the {} bytes \
             written (object form {form}, re-checked {recheck})",
            now.len(),
            content.len()
        );
    }
    if rechecked_and_rewritten {
        let u = undone
            .unwrap_or_else(|e| panic!("undo failed after the bad object was rewritten: {e:?}"));
        assert!(u.restored.contains(&path), "undo did not restore brief.txt");
    }
    drop(matter);
});
