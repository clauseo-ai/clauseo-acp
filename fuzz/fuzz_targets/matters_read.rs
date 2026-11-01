//! matters: reads of `.clauseo/matter.json` and of the stat cache `history.git/index`, through
//! `Matter::open`.
//!
//! Items driven: `Matter::create` (setup), `Matter::open` and `Matter::timeline`. Opening runs
//! the matter.json check (matter/json.rs) and the stat-cache load (history/index_file.rs, reached
//! from the git store when the matter is loaded).
//!
//! One matter per process, created with `Matter::create` in a scratch folder under the temp dir
//! (see scratch.rs). Each iteration writes the fuzz payload to one of the two files (restoring the
//! other to its pristine bytes), then calls `Matter::open`.
//!
//! Input: byte 0 = selector, rest = payload.
//!   sel & 3 == 0: payload is matter.json; the index is restored to its pristine bytes.
//!   sel & 3 == 1: payload is the index file, as it is; matter.json is restored.
//!   sel & 3 == 2: payload is the index file with a valid SHA-1 trailer appended, so the
//!                 checksum passes and the entries are decoded.
//!   sel & 3 == 3: payload is matter.json and the index is removed (a missing cache).
//!
//! Invariants:
//!   - matter.json: `Matter::open` succeeds if and only if the payload parses as
//!     `{matter_id, created_at, version?}` (serde_json; unknown keys are ignored) and names this
//!     matter. Otherwise it is refused.
//!   - index: `Matter::open` always succeeds. An index that does not read is logged and rebuilt,
//!     and the timeline keeps the one checkpoint that `create` made.
#![no_main]
#![expect(
    clippy::disallowed_methods,
    reason = "fuzz harness, not app code: fuzz_target! and its scratch folder write with std::fs"
)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use clauseo_matters::{Limits, Marker, Matter, NewMatter, OsProbe, Retention, Settings};
use clauseo_protocol::{CheckpointId, MatterId, Timestamp};
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

mod scratch;

struct Fixture {
    root: PathBuf,
    folder: PathBuf,
    id: MatterId,
    matter_json: Vec<u8>,
    index_path: PathBuf,
    index: Vec<u8>,
    timeline_len: usize,
}

fn settings(root: &Path) -> Settings {
    Settings {
        matters_root: root.join("homes"),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
    }
}

fn fixture() -> &'static Fixture {
    static CELL: OnceLock<Fixture> = OnceLock::new();
    CELL.get_or_init(|| {
        let root = scratch::create("matters");
        std::fs::create_dir_all(root.join("homes")).unwrap();
        let folder = root.join("matter");
        let id: MatterId = "01900000-0000-7000-8000-000000000001".parse().unwrap();
        let matter = Matter::create(
            settings(&root),
            NewMatter {
                id,
                folder: folder.clone(),
                created_at: Timestamp::new(1_760_000_000_000).unwrap(),
                marker: Marker::None,
                checkpoint: "01900000-0000-7000-8000-000000000002"
                    .parse::<CheckpointId>()
                    .unwrap(),
            },
        )
        .expect("matter create");
        let timeline_len = matter.timeline().len();
        drop(matter);
        std::fs::write(folder.join("brief.txt"), b"Heads of terms\n").unwrap();
        let matter_json = std::fs::read(folder.join(".clauseo/matter.json")).unwrap();
        let index_path = root
            .join("homes")
            .join(id.to_string())
            .join("history.git")
            .join("index");
        let index = std::fs::read(&index_path).unwrap_or_default();
        Fixture {
            root,
            folder,
            id,
            matter_json,
            index_path,
            index,
            timeline_len,
        }
    })
}

/// The fields matter.json must have, with their types: only `matter_id` is compared, the other
/// two must just decode.
#[derive(Deserialize)]
struct MatterJson {
    #[serde(default, rename = "version")]
    _version: u32,
    matter_id: MatterId,
    #[serde(rename = "created_at")]
    _created_at: Timestamp,
}

fn with_trailer(payload: &[u8]) -> Vec<u8> {
    let mut out = payload.to_vec();
    out.extend_from_slice(&sha1_smol::Sha1::from(payload).digest().bytes());
    out
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, payload)) = data.split_first() else {
        return;
    };
    let fx = fixture();
    let json_path = fx.folder.join(".clauseo/matter.json");
    let mode = selector & 3;
    let (json, index): (&[u8], Option<Vec<u8>>) = match mode {
        0 => (payload, Some(fx.index.clone())),
        1 => (&fx.matter_json, Some(payload.to_vec())),
        2 => (&fx.matter_json, Some(with_trailer(payload))),
        _ => (payload, None),
    };
    std::fs::write(&json_path, json).unwrap();
    match &index {
        Some(bytes) => std::fs::write(&fx.index_path, bytes).unwrap(),
        None => {
            let _ = std::fs::remove_file(&fx.index_path);
        }
    }
    let opened = Matter::open(settings(&fx.root), fx.id, &fx.folder);
    match mode {
        0 | 3 => {
            let names_us =
                serde_json::from_slice::<MatterJson>(payload).is_ok_and(|j| j.matter_id == fx.id);
            match &opened {
                Ok(_) => assert!(names_us, "opened with a matter.json that does not name it"),
                Err(e) => assert!(!names_us, "refused a matter.json that names it: {e:?}"),
            }
        }
        _ => {
            let matter = opened.unwrap_or_else(|e| panic!("a bad stat cache failed open: {e:?}"));
            assert_eq!(matter.timeline().len(), fx.timeline_len);
        }
    }
});
