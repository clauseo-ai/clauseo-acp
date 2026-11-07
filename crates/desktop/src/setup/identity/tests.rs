//! The identity check's parts: each verdict's refusal, the record's round trip, a record not
//! written when unchanged, and what cannot be read.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_agents::AccountHash;
use clauseo_protocol::{AgentId, ErrorCode};
use clauseo_settings::Files;

use super::Verdict;
use super::store::{Record, Store};
use crate::bridge::lock;

/// Files in memory, counting writes; a read of `broken` fails.
#[derive(Default)]
struct Counted {
    files: Mutex<BTreeMap<PathBuf, Vec<u8>>>,
    writes: Mutex<usize>,
    broken: bool,
}

impl Files for Counted {
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        if self.broken {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        }
        Ok(lock(&self.files).get(path).cloned())
    }
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        *lock(&self.writes) += 1;
        lock(&self.files).insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

#[test]
fn each_verdict_has_its_refusal() {
    let table = [
        (Verdict::Idle, None),
        (Verdict::Same, None),
        (Verdict::Due, Some(ErrorCode::AgentIdentityChecking)),
        (Verdict::Checking, Some(ErrorCode::AgentIdentityChecking)),
        (Verdict::Unknown, Some(ErrorCode::AgentIdentityUnknown)),
        (Verdict::Changed, Some(ErrorCode::SigninAccountChanged)),
    ];
    for (verdict, code) in table {
        assert_eq!(verdict.refusal(), code, "{verdict:?}");
    }
}

#[test]
fn the_record_round_trips_and_is_not_written_when_unchanged() {
    let files = Arc::new(Counted::default());
    let store = Store::at(
        Path::new("/data"),
        Arc::clone(&files) as Arc<dyn Files + Send + Sync>,
    );
    assert_eq!(
        store.load(),
        Ok(Record::default()),
        "no file: nothing recorded"
    );
    let accounts = BTreeMap::from([(codex(), AccountHash::of("a@b.example").unwrap())]);
    let recorded = Record {
        accounts,
        signed_out: BTreeSet::new(),
    };
    store.save(&recorded).unwrap();
    store.save(&recorded).unwrap();
    assert_eq!(*lock(&files.writes), 1, "the same record is written once");
    let path = Path::new("/data/agents/identity.json");
    let text = String::from_utf8(lock(&files.files)[path].clone()).unwrap();
    assert!(!text.contains("a@b.example"), "{text}");
    let again = Store::at(Path::new("/data"), files as Arc<dyn Files + Send + Sync>);
    assert_eq!(again.load(), Ok(recorded));
}

#[test]
fn a_record_that_cannot_be_read_or_parsed_is_an_error() {
    let broken = Arc::new(Counted {
        broken: true,
        ..Counted::default()
    });
    let store = Store::at(Path::new("/data"), broken as Arc<dyn Files + Send + Sync>);
    assert_eq!(store.load(), Err(io::ErrorKind::PermissionDenied));
    for text in [
        "{ nope",
        r#"{"codex-acp": "not-a-hash"}"#,
        r#"{"NOT AN ID": "aa"}"#,
        r#"{"codex-acp": {"signed_out": true}}"#,
        r#"{"codex-acp": {"account": "not-a-hash", "signed_out": true}}"#,
        &format!(r#"{{"codex-acp": {{"account": "{HEX}", "signed_out": "yes"}}}}"#),
        &format!(r#"{{"codex-acp": {{"account": "{HEX}", "other": 1}}}}"#),
        r#"{"codex-acp": 7}"#,
    ] {
        let files = Arc::new(Counted::default());
        let path = PathBuf::from("/data/agents/identity.json");
        lock(&files.files).insert(path, text.as_bytes().to_vec());
        let store = Store::at(Path::new("/data"), files as Arc<dyn Files + Send + Sync>);
        assert_eq!(store.load(), Err(io::ErrorKind::InvalidData), "{text}");
    }
}

/// A hash's hex, for the record's text.
const HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn a_signed_out_mark_round_trips_beside_its_account_and_the_old_form_still_reads() {
    let files = Arc::new(Counted::default());
    let store = Store::at(
        Path::new("/data"),
        Arc::clone(&files) as Arc<dyn Files + Send + Sync>,
    );
    let hash = AccountHash::from_hex(HEX).unwrap();
    let record = Record {
        accounts: BTreeMap::from([(codex(), hash.clone())]),
        signed_out: BTreeSet::from([codex()]),
    };
    store.save(&record).unwrap();
    let path = Path::new("/data/agents/identity.json");
    let text = String::from_utf8(lock(&files.files)[path].clone()).unwrap();
    assert!(text.contains(r#""signed_out": true"#), "{text}");
    let again = Store::at(
        Path::new("/data"),
        Arc::clone(&files) as Arc<dyn Files + Send + Sync>,
    );
    assert_eq!(again.load(), Ok(record));
    // The form before the mark, an id to its hash: recorded, not signed out.
    lock(&files.files).insert(
        path.to_path_buf(),
        format!(r#"{{"codex-acp": "{HEX}"}}"#).into_bytes(),
    );
    let old = Store::at(Path::new("/data"), files as Arc<dyn Files + Send + Sync>);
    let read = old.load().unwrap();
    assert_eq!(read.accounts, BTreeMap::from([(codex(), hash)]));
    assert!(read.signed_out.is_empty());
}
