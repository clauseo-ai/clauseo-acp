//! Tests of the version-history tool against real matters on disk, read through a stand-in for
//! the matter's thread that runs each read at once on the caller's thread.

mod caps;
mod concurrency;
mod continuity;
mod docx;
mod list;
mod read;
mod surface;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_matters::{
    DEFAULT_MARKER, Limits, Matter, NewMatter, Restore, RestoreFiles, TurnMeta, UndoRequest,
};
use clauseo_protocol::{
    CheckpointId, FileChoice, FileDecision, MatterId, MatterPath, ThreadId, Timestamp, TurnId,
};
use serde_json::{Value, json};

use super::{
    Logged, MatterRead, Opened, Unreachable, VersionsBackend, VersionsConfig, VersionsSource,
};
use crate::config::SessionKey;
use crate::matter_worker::matter_settings;
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// A valid v7 id, distinct per `n`.
pub(super) fn v7(n: u32) -> uuid::Uuid {
    let mut random = [0_u8; 10];
    for (byte, from) in random.iter_mut().zip(n.to_be_bytes()) {
        *byte = from;
    }
    uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &random).into_uuid()
}

pub(super) fn checkpoint(n: u32) -> CheckpointId {
    CheckpointId::new(v7(n)).unwrap()
}

/// The matters the stand-in holds, and what their log says.
#[derive(Default)]
pub(super) struct Source {
    pub(super) matters: Mutex<Vec<Matter>>,
    pub(super) logged: Mutex<Logged>,
    /// The size cap reads get, when not the default one.
    pub(super) cap: Mutex<Option<u64>>,
}

impl VersionsSource for Source {
    fn on_matter(&self, matter: MatterId, read: MatterRead) -> Result<(), Unreachable> {
        let matters = self.matters.lock().unwrap();
        let held = matters.iter().find(|held| held.id() == matter);
        let cap = self
            .cap
            .lock()
            .unwrap()
            .unwrap_or(Limits::DEFAULT.effective_cap());
        read.run(held.map(|matter| Opened { matter, cap }));
        Ok(())
    }

    fn logged(&self, _matter: MatterId, versions: &[CheckpointId]) -> Logged {
        let logged = self.logged.lock().unwrap();
        Logged {
            at: logged
                .at
                .iter()
                .filter(|(id, _)| versions.contains(id))
                .map(|(id, at)| (*id, *at))
                .collect(),
            restored_from: logged
                .restored_from
                .iter()
                .filter(|(id, _)| versions.contains(id))
                .map(|(id, from)| (*id, *from))
                .collect(),
        }
    }
}

/// A data folder with matters in it and a session of the first.
pub(super) struct Fixture {
    pub(super) dir: TempDir,
    pub(super) source: Arc<Source>,
    pub(super) session: SessionKey,
    next: u32,
}

impl Fixture {
    /// A matter (`matter`) with its root import checkpoint (id 1); the session's thread is 2.
    pub(super) fn new(name: &str) -> Self {
        let dir = TempDir::new(name);
        let mut fixture = Self {
            dir,
            source: Arc::new(Source::default()),
            session: SessionKey {
                matter: MatterId::new(v7(100)).unwrap(),
                thread: ThreadId::new(v7(2)).unwrap(),
                launch: 1,
            },
            next: 1000,
        };
        fixture.add_matter(v7(100), "matter", 1);
        fixture
    }

    /// Creates matter `id` in folder `name` with root checkpoint `root`.
    pub(super) fn add_matter(&mut self, id: uuid::Uuid, name: &str, root: u32) {
        let matter = Matter::create(
            matter_settings(&self.data()),
            NewMatter {
                id: MatterId::new(id).unwrap(),
                folder: self.dir.path().join(name),
                created_at: Timestamp::new(1_790_000_000_000).unwrap(),
                marker: DEFAULT_MARKER,
                checkpoint: checkpoint(root),
            },
        )
        .unwrap();
        self.source.matters.lock().unwrap().push(matter);
    }

    pub(super) fn data(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    /// The session's matter's folder.
    pub(super) fn folder(&self) -> PathBuf {
        self.dir.path().join("matter")
    }

    /// Writes `path` in folder `name`.
    pub(super) fn put_in(&self, name: &str, path: &str, bytes: &[u8]) {
        let abs = self.dir.path().join(name).join(path);
        write::create_dir_all(abs.parent().unwrap()).unwrap();
        write::write_file(&abs, bytes).unwrap();
    }

    pub(super) fn put(&self, path: &str, bytes: &[u8]) {
        self.put_in("matter", path, bytes);
    }

    pub(super) fn remove(&self, path: &str) {
        write::remove_file(&self.folder().join(path)).unwrap();
    }

    pub(super) fn rename(&self, from: &str, to: &str) {
        write::create_dir_all(self.folder().join(to).parent().unwrap()).unwrap();
        write::rename(&self.folder().join(from), &self.folder().join(to)).unwrap();
    }

    fn meta(&mut self, n: u32, thread: ThreadId) -> TurnMeta {
        self.next += 1;
        TurnMeta {
            checkpoint: checkpoint(n),
            thread,
            turn: TurnId::new(v7(self.next)).unwrap(),
            at: Timestamp::new(1_790_000_000_000 + i64::from(n) * 1000).unwrap(),
        }
    }

    /// A `before-turn` checkpoint `n` of matter `matter_index`'s folder as it is (it must
    /// differ from the last).
    pub(super) fn checkpoint_of(&mut self, matter_index: usize, n: u32) -> CheckpointId {
        let meta = self.meta(n, self.session.thread);
        let mut matters = self.source.matters.lock().unwrap();
        let taken = matters[matter_index].before_turn(&meta).unwrap();
        assert_eq!(
            taken.outcome.id(),
            checkpoint(n),
            "checkpoint {n} was reused"
        );
        checkpoint(n)
    }

    /// A `before-turn` checkpoint `n` of the session's matter.
    pub(super) fn checkpoint(&mut self, n: u32) -> CheckpointId {
        self.checkpoint_of(0, n)
    }

    /// An `after-turn` checkpoint `n` of the session's matter, by `thread`'s agent.
    pub(super) fn after_turn(&mut self, n: u32, thread: ThreadId) -> CheckpointId {
        let meta = self.meta(n, thread);
        let mut matters = self.source.matters.lock().unwrap();
        let taken = matters[0].after_turn(&meta, &BTreeSet::new()).unwrap();
        assert_eq!(
            taken.outcome.id(),
            checkpoint(n),
            "checkpoint {n} was reused"
        );
        checkpoint(n)
    }

    /// Restores `files` of version `from` as the lawyer does from Versions: an `outside-edits`
    /// checkpoint `outside` when the folder changed, then the `restore` checkpoint `n`.
    pub(super) fn restore(&mut self, from: CheckpointId, files: &[&str], outside: u32, n: u32) {
        let paths: Vec<MatterPath> = files.iter().map(|file| path(file)).collect();
        let mut matters = self.source.matters.lock().unwrap();
        matters[0]
            .restore(&Restore {
                undoes: None,
                from,
                files: RestoreFiles::Only(&paths),
                outside_edits: checkpoint(outside),
                checkpoint: checkpoint(n),
                at: Timestamp::new(1_790_000_000_000 + i64::from(n) * 1000).unwrap(),
            })
            .unwrap();
    }

    /// Undoes the step from `before` to `after` for `files`, as the lawyer does: an
    /// `outside-edits` checkpoint `outside` when the folder changed, then the `undo` checkpoint
    /// `n`.
    pub(super) fn undo(
        &mut self,
        (before, after): (u32, u32),
        files: &[&str],
        outside: u32,
        n: u32,
    ) {
        let decisions: Vec<FileDecision> = files
            .iter()
            .map(|file| FileDecision {
                path: path(file),
                choice: FileChoice::Undo,
            })
            .collect();
        let meta = self.meta(n, self.session.thread);
        let mut matters = self.source.matters.lock().unwrap();
        matters[0]
            .undo(&UndoRequest {
                before: checkpoint(before),
                after: checkpoint(after),
                decisions: &decisions,
                thread: Some(meta.thread),
                turn: Some(meta.turn),
                outside_edits: checkpoint(outside),
                checkpoint: checkpoint(n),
                at: meta.at,
            })
            .unwrap();
    }

    /// A checkpoint `n` that also versions `old`, files under excluded folders, as a build from
    /// before the dot-folder rule did.
    pub(super) fn checkpoint_with_old(&mut self, n: u32, old: &[&str]) -> CheckpointId {
        let meta = self.meta(n, self.session.thread);
        let mut matters = self.source.matters.lock().unwrap();
        clauseo_matters::test_support::checkpoint_with_old_files(&mut matters[0], &meta, old)
            .unwrap();
        checkpoint(n)
    }

    /// The backend of the session, with the deleted-file limit as the constant sets it.
    pub(super) fn backend(&self) -> VersionsBackend {
        VersionsBackend::new(self.config(None))
    }

    /// The backend with deleted files readable or not.
    pub(super) fn backend_deleted(&self, readable: bool) -> VersionsBackend {
        VersionsBackend::with_deleted_readable(self.config(None), readable)
    }

    pub(super) fn config(&self, stetdoc: Option<PathBuf>) -> VersionsConfig {
        VersionsConfig {
            session: self.session,
            source: Arc::clone(&self.source) as Arc<dyn VersionsSource>,
            clock: Arc::new(crate::clock::SystemClock),
            data_dir: self.data(),
            stetdoc,
        }
    }
}

/// `tools/call` of `tool` with `args`: its `result`.
pub(super) fn call(backend: &VersionsBackend, tool: &str, args: Value) -> Value {
    let mut message = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": { "name": tool },
    });
    message["params"]["arguments"] = args;
    let answer = backend.answer(&message).unwrap();
    assert_eq!(answer["id"], 7, "{answer}");
    answer["result"].clone()
}

/// The answer of a call that must succeed, read back from its text into the fields it names
/// (as JSON, so tests compare them): the result holds one text block and no
/// `structuredContent`, so the answer reaches the model once.
pub(super) fn ok(result: &Value) -> Value {
    assert_eq!(result["isError"], false, "{result}");
    assert!(result.get("structuredContent").is_none(), "{result}");
    let content = result["content"].as_array().unwrap();
    assert_eq!(content.len(), 1, "{result}");
    assert_eq!(content[0]["type"], "text", "{result}");
    let text = content[0]["text"].as_str().unwrap();
    if text.starts_with("versions: ") {
        parse_list(text)
    } else {
        parse_read(text)
    }
}

/// `read_version`'s answer: the header line's fields, and the page: all that follows the blank
/// line after it (the shape test checks the whole text).
fn parse_read(text: &str) -> Value {
    let (header, page) = text.split_once("\n\n").unwrap();
    assert!(!header.contains('\n'), "{header}");
    let (fields, path) = header.split_once("; path: ").unwrap();
    let mut answer = serde_json::Map::new();
    for field in fields.split("; ") {
        let (key, value) = field.split_once(": ").unwrap();
        let value = match (key, value) {
            ("next_offset", "end") => continue,
            ("at", "unknown") => Value::Null,
            ("offset" | "next_offset" | "total_bytes", number) => {
                json!(number.parse::<u64>().unwrap())
            }
            (_, other) => json!(other),
        };
        answer.insert(key.to_owned(), value);
    }
    answer.insert("path".to_owned(), json!(path));
    answer.insert("text".to_owned(), json!(page));
    Value::Object(answer)
}

/// `list_versions`' answer: the count and `next_before` lines, then one block per version.
fn parse_list(text: &str) -> Value {
    let mut blocks = text.split("\n\n");
    let mut head = blocks.next().unwrap().lines();
    let count: usize = head
        .next()
        .unwrap()
        .strip_prefix("versions: ")
        .unwrap()
        .strip_suffix(", newest first")
        .unwrap()
        .parse()
        .unwrap();
    let next = head.next().unwrap().strip_prefix("next_before: ").unwrap();
    assert_eq!(head.next(), None);
    let versions: Vec<Value> = blocks.map(parse_version).collect();
    assert_eq!(versions.len(), count, "{text}");
    let mut page = json!({ "versions": versions });
    if !next.starts_with("none") {
        page["next_before"] = json!(next);
    }
    page
}

fn parse_version(block: &str) -> Value {
    let mut version = serde_json::Map::new();
    let mut files: Vec<Value> = Vec::new();
    for line in block.lines() {
        if let Some(from) = line.strip_prefix("    from: ") {
            files.last_mut().unwrap()["from"] = json!(from);
        } else if let Some(file) = line.strip_prefix("  ") {
            let (change, path) = file.split_once(": ").unwrap();
            files.push(json!({ "path": path, "change": change }));
        } else if line != "files:" {
            let (key, value) = line.split_once(": ").unwrap();
            let value = match (key, value) {
                ("at", "unknown") => Value::Null,
                ("this_conversation", yes) => json!(yes == "yes"),
                ("files_total", number) => json!(number.parse::<u64>().unwrap()),
                (_, other) => json!(other),
            };
            version.insert(key.to_owned(), value);
        }
    }
    version.insert("files".to_owned(), Value::Array(files));
    Value::Object(version)
}

/// The refusal sentence of a call that must be refused.
pub(super) fn refused(result: &Value) -> &str {
    assert_eq!(result["isError"], true, "{result}");
    assert!(result.get("structuredContent").is_none(), "{result}");
    result["content"][0]["text"].as_str().unwrap()
}

/// `read_version` of `path` at `version`.
pub(super) fn read(backend: &VersionsBackend, version: CheckpointId, path: &str) -> Value {
    call(
        backend,
        "read_version",
        json!({ "version": version.to_string(), "path": path }),
    )
}

/// Every file under `root` with its bytes, sorted: what a test compares before and after.
pub(super) fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, Option<std::time::SystemTime>)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                pending.push(path.clone());
                found.push((path, Vec::new(), metadata.modified().ok()));
            } else {
                let bytes = std::fs::read(&path).unwrap();
                found.push((path, bytes, metadata.modified().ok()));
            }
        }
    }
    found.sort();
    found
}

pub(super) fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}
