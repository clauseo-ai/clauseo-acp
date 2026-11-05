//! The matter SIDs' file: one random SID per matter, the restricting SID that keeps one matter's
//! agent out of another's folders (S15, "The token"), kept in `<data_dir>\sandbox\matters.json`
//! (matter id to SID). A matter's SID is made at its first launch, from the OS's randomness, and
//! kept for good: nothing here removes one, and only `forget_matter` (S15 PR3, S15a) will. A SID
//! is never handed to a second matter, and never one of this machine's accounts or their domain
//! ([`Reserved`]).
//!
//! Built on every OS so its tests run everywhere; only Windows uses it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::MatterId;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::sids::{Reserved, Sid};
use super::write;
use crate::SetupError;

/// The folder under the data folder that holds the sandbox's records.
pub const SANDBOX_DIR: &str = "sandbox";
/// The matter SIDs' file, in [`SANDBOX_DIR`].
pub const MATTERS_FILE: &str = "matters.json";
/// The lock file every change of [`MATTERS_FILE`] holds, in [`SANDBOX_DIR`].
pub const MATTERS_LOCK: &str = "matters.lock";
/// The version `matters.json` is written at; a file at another version is refused, not
/// rewritten.
const MATTERS_VERSION: u32 = 1;

/// `matters.json` as it is written.
#[derive(Serialize)]
struct MattersOut<'a> {
    version: u32,
    matters: &'a BTreeMap<MatterId, Sid>,
}

/// `matters.json` as it is read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MattersIn {
    version: u32,
    matters: UniqueMatters,
}

/// The matters map, refused when it names one matter twice (JSON allows a repeated key, and a
/// plain map would keep the last silently).
struct UniqueMatters(BTreeMap<MatterId, Sid>);

impl<'de> Deserialize<'de> for UniqueMatters {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries;

        impl<'de> Visitor<'de> for Entries {
            type Value = UniqueMatters;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map of matter ids to SIDs")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<UniqueMatters, A::Error> {
                let mut matters = BTreeMap::new();
                while let Some((matter, sid)) = map.next_entry::<MatterId, Sid>()? {
                    if matters.insert(matter, sid).is_some() {
                        return Err(de::Error::custom(format!(
                            "matter {matter} is listed twice"
                        )));
                    }
                }
                Ok(UniqueMatters(matters))
            }
        }

        deserializer.deserialize_map(Entries)
    }
}

/// The matter SIDs of one data folder.
#[derive(Debug)]
pub struct MatterSids {
    folder: PathBuf,
    reserved: Reserved,
    sids: BTreeMap<MatterId, Sid>,
    /// How long [`MatterSids::sid_for`] waits for `matters.lock`; `None` is
    /// [`write::LOCK_WAIT`].
    lock_wait: Option<Duration>,
}

/// The SID of `matter` in `data_dir`'s `matters.json`, made and saved first if the matter has
/// none ([`MatterSids::sid_for`]).
pub fn matter_sid(data_dir: &Path, matter: &MatterId) -> Result<Sid, SetupError> {
    MatterSids::load(data_dir)?.sid_for(matter)
}

impl MatterSids {
    /// Loads `<data_dir>\sandbox\matters.json` against this machine's accounts
    /// ([`Reserved::of_this_machine`] on Windows; none elsewhere, where only tests load it).
    pub fn load(data_dir: &Path) -> Result<MatterSids, SetupError> {
        #[cfg(windows)]
        let reserved = Reserved::of_this_machine()?;
        #[cfg(not(windows))]
        let reserved = Reserved::default();
        MatterSids::load_with(data_dir, reserved)
    }

    /// Loads `<data_dir>\sandbox\matters.json`, reading only; no file is no matters. A file
    /// that can't be read or parsed, is at another version, names a matter twice, holds a SID
    /// not of the matter form or one `reserved` refuses, or holds one SID twice is an error, and
    /// is left as it is: rewriting it would lose matters' SIDs, and with them the way to remove
    /// their permission entries.
    pub fn load_with(data_dir: &Path, reserved: Reserved) -> Result<MatterSids, SetupError> {
        let folder = data_dir.join(SANDBOX_DIR);
        let sids = read(&folder.join(MATTERS_FILE), &reserved)?;
        Ok(MatterSids {
            folder,
            reserved,
            sids,
            lock_wait: None,
        })
    }

    /// Waits up to `wait` for `matters.lock` instead of [`write::LOCK_WAIT`] (10 s).
    pub fn with_lock_wait(mut self, wait: Duration) -> MatterSids {
        self.lock_wait = Some(wait);
        self
    }

    /// The SID `matter` had when this was loaded or last changed, if any.
    pub fn get(&self, matter: &MatterId) -> Option<&Sid> {
        self.sids.get(matter)
    }

    /// How many matters have a SID.
    pub fn len(&self) -> usize {
        self.sids.len()
    }

    /// Whether no matter has a SID.
    pub fn is_empty(&self) -> bool {
        self.sids.is_empty()
    }

    /// The SID of `matter`. Makes the sandbox folder if missing (`write::make_sandbox_dir`),
    /// then holds `matters.lock` (an exclusive OS file lock, so no other process or instance
    /// changes the file meanwhile; waiting at most 10 s, or [`MatterSids::with_lock_wait`], then
    /// an error naming the lock) and reads the file again inside it, so a matter another
    /// instance added since this one loaded is kept. A matter without a SID gets a new random one
    /// that no matter holds and [`Reserved`] admits, and the file is saved with it before it is
    /// returned; if the save fails, the matter has no SID.
    pub fn sid_for(&mut self, matter: &MatterId) -> Result<Sid, SetupError> {
        let failed =
            |why: io::Error| SetupError::Failed(format!("{}: {why}", self.folder.display()));
        write::make_sandbox_dir(&self.folder).map_err(failed)?;
        let lock = self.folder.join(MATTERS_LOCK);
        let _held = match self.lock_wait {
            None => write::lock_exclusive(&lock),
            Some(wait) => write::lock_exclusive_for(&lock, wait),
        }
        .map_err(|error| SetupError::Failed(format!("matter SIDs: {error}")))?;
        let file = self.folder.join(MATTERS_FILE);
        self.sids = read(&file, &self.reserved)?;
        if let Some(sid) = self.sids.get(matter) {
            return Ok(sid.clone());
        }
        let sid = loop {
            let sid = Sid::random_matter()
                .map_err(|why| SetupError::Failed(format!("matter SID: {why}")))?;
            if self.reserved.admits(&sid) && !self.sids.values().any(|held| *held == sid) {
                break sid;
            }
        };
        let mut sids = self.sids.clone();
        sids.insert(*matter, sid.clone());
        save(&file, &sids)?;
        self.sids = sids;
        Ok(sid)
    }
}

/// Reads and checks the file `path`, as [`MatterSids::load_with`] describes.
fn read(path: &Path, reserved: &Reserved) -> Result<BTreeMap<MatterId, Sid>, SetupError> {
    let failed = |why: String| SetupError::Failed(format!("{}: {why}", path.display()));
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(failed(error.to_string())),
    };
    let file: MattersIn =
        serde_json::from_slice(&bytes).map_err(|error| failed(error.to_string()))?;
    if file.version != MATTERS_VERSION {
        return Err(failed(format!(
            "version {}, not {MATTERS_VERSION}",
            file.version
        )));
    }
    let mut seen = BTreeSet::new();
    for (matter, sid) in &file.matters.0 {
        if !sid.is_matter_form() {
            return Err(failed(format!("{matter}'s SID {sid} is not a matter SID")));
        }
        if !reserved.admits(sid) {
            return Err(failed(format!(
                "{matter}'s SID {sid} is or shares a domain with an account of this machine"
            )));
        }
        if !seen.insert(sid) {
            return Err(failed(format!("{sid} is held by two matters")));
        }
    }
    Ok(file.matters.0)
}

/// Writes `sids` to `path` atomically.
fn save(path: &Path, sids: &BTreeMap<MatterId, Sid>) -> Result<(), SetupError> {
    let failed = |why: String| SetupError::Failed(format!("{}: {why}", path.display()));
    let file = MattersOut {
        version: MATTERS_VERSION,
        matters: sids,
    };
    let mut bytes = serde_json::to_vec_pretty(&file).map_err(|error| failed(error.to_string()))?;
    bytes.push(b'\n');
    write::write_atomic(path, &bytes).map_err(|error| failed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    /// A fresh matter id: a v7 UUID from random bytes, enough for distinct test matters.
    fn matter() -> MatterId {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).unwrap();
        bytes[6] = (bytes[6] & 0x0f) | 0x70;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex = |range: std::ops::Range<usize>| -> String {
            bytes[range]
                .iter()
                .flat_map(|byte| [byte >> 4, byte & 0xf])
                .map(|nibble| char::from_digit(u32::from(nibble), 16).unwrap())
                .collect()
        };
        let text = format!(
            "{}-{}-{}-{}-{}",
            hex(0..4),
            hex(4..6),
            hex(6..8),
            hex(8..10),
            hex(10..16)
        );
        text.parse().unwrap()
    }

    #[test]
    fn two_matters_get_different_sids_and_keep_them_across_reloads() {
        let temp = TempDir::new("matter-sids");
        let (a, b) = (matter(), matter());
        let sid_a = matter_sid(temp.path(), &a).unwrap();
        let sid_b = matter_sid(temp.path(), &b).unwrap();
        assert_ne!(sid_a, sid_b);
        assert_eq!(matter_sid(temp.path(), &a).unwrap(), sid_a);
        let loaded = MatterSids::load(temp.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get(&a), Some(&sid_a));
        assert_eq!(loaded.get(&b), Some(&sid_b));
    }

    #[test]
    fn the_file_is_replaced_whole_and_reloads() {
        let temp = TempDir::new("matter-sids-file");
        let a = matter();
        let mut sids = MatterSids::load(temp.path()).unwrap();
        assert!(sids.is_empty());
        let sid = sids.sid_for(&a).unwrap();
        let folder = temp.path().join(SANDBOX_DIR);
        let text = std::fs::read_to_string(folder.join(MATTERS_FILE)).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["matters"][a.to_string()], sid.as_str());
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [MATTERS_FILE, MATTERS_LOCK],
            "no temporary file is left"
        );
        let b = matter();
        let sid_b = sids.sid_for(&b).unwrap();
        let again = MatterSids::load(temp.path()).unwrap();
        assert_eq!((again.get(&a), again.get(&b)), (Some(&sid), Some(&sid_b)));
    }

    #[test]
    fn a_bad_file_is_refused_and_left_as_it_is() {
        let temp = TempDir::new("matter-sids-bad");
        let a = matter().to_string();
        let b = matter().to_string();
        let sid = "S-1-5-21-1-2-3-1000";
        for bad in [
            "not json".to_owned(),
            r#"{"version":2,"matters":{}}"#.to_owned(),
            format!(r#"{{"version":1,"matters":{{"{a}":"S-1-5-32-544"}}}}"#),
            format!(r#"{{"version":1,"matters":{{"{a}":"{sid}","{b}":"{sid}"}}}}"#),
            format!(r#"{{"version":1,"matters":{{"not-a-matter":"{sid}"}}}}"#),
            r#"{"version":1,"matters":{},"extra":1}"#.to_owned(),
        ] {
            let path = temp.file(&format!("{SANDBOX_DIR}/{MATTERS_FILE}"), &bad);
            assert!(MatterSids::load(temp.path()).is_err(), "{bad}");
            assert!(matter_sid(temp.path(), &matter()).is_err(), "{bad}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
        }
    }

    #[test]
    fn a_repeated_matter_is_refused() {
        let temp = TempDir::new("matter-sids-repeat");
        let a = matter();
        let path = format!("{SANDBOX_DIR}/{MATTERS_FILE}");
        // The review's repro, without a version, and the same with one.
        let entries = format!(r#""{a}":"S-1-5-21-1-2-3-1000","{a}":"S-1-5-21-4-5-6-1000""#);
        temp.file(&path, &format!(r#"{{"matters":{{{entries}}}}}"#));
        assert!(MatterSids::load(temp.path()).is_err());
        temp.file(
            &path,
            &format!(r#"{{"version":1,"matters":{{{entries}}}}}"#),
        );
        let Err(SetupError::Failed(why)) = MatterSids::load(temp.path()) else {
            panic!("a repeated matter loaded");
        };
        assert!(why.contains("listed twice"), "{why}");
    }

    #[test]
    fn a_sid_of_or_beside_a_machine_account_is_refused() {
        let temp = TempDir::new("matter-sids-reserved");
        let a = matter();
        let lawyer = Sid::parse("S-1-5-21-7-8-9-1001").unwrap();
        let reserved = || Reserved::new([lawyer.clone()]);
        let path = format!("{SANDBOX_DIR}/{MATTERS_FILE}");
        for sid in ["S-1-5-21-7-8-9-1001", "S-1-5-21-7-8-9-424242"] {
            temp.file(
                &path,
                &format!(r#"{{"version":1,"matters":{{"{a}":"{sid}"}}}}"#),
            );
            assert!(
                MatterSids::load_with(temp.path(), reserved()).is_err(),
                "{sid}"
            );
            assert!(
                MatterSids::load_with(temp.path(), Reserved::default()).is_ok(),
                "{sid}"
            );
        }
        temp.file(
            &path,
            &format!(r#"{{"version":1,"matters":{{"{a}":"S-1-5-21-7-8-10-1001"}}}}"#),
        );
        assert!(MatterSids::load_with(temp.path(), reserved()).is_ok());
    }

    #[test]
    fn new_sids_are_admitted_by_the_reservations() {
        let temp = TempDir::new("matter-sids-new-reserved");
        let lawyer = Sid::parse("S-1-5-21-7-8-9-1001").unwrap();
        let reserved = Reserved::new([lawyer]);
        let mut sids = MatterSids::load_with(temp.path(), reserved.clone()).unwrap();
        for _ in 0..50 {
            let sid = sids.sid_for(&matter()).unwrap();
            assert!(reserved.admits(&sid), "{sid}");
        }
    }

    #[test]
    fn two_instances_adding_matters_keep_both() {
        let temp = TempDir::new("matter-sids-two");
        let mut first = MatterSids::load(temp.path()).unwrap();
        let mut second = MatterSids::load(temp.path()).unwrap();
        let (a, b) = (matter(), matter());
        let sid_a = first.sid_for(&a).unwrap();
        let sid_b = second.sid_for(&b).unwrap();
        // The second re-read the file under the lock, so it saw A before adding B.
        assert_eq!(second.get(&a), Some(&sid_a));
        let again = MatterSids::load(temp.path()).unwrap();
        assert_eq!((again.get(&a), again.get(&b)), (Some(&sid_a), Some(&sid_b)));
        // And the first, asked for A again, keeps its SID.
        assert_eq!(first.sid_for(&a).unwrap(), sid_a);
    }

    #[test]
    fn a_held_lock_makes_sid_for_give_up_after_its_wait() {
        let temp = TempDir::new("matter-sids-held");
        let folder = temp.path().join(SANDBOX_DIR);
        write::make_sandbox_dir(&folder).unwrap();
        let _held = write::lock_exclusive(&folder.join(MATTERS_LOCK)).unwrap();
        let mut sids = MatterSids::load(temp.path())
            .unwrap()
            .with_lock_wait(Duration::from_millis(200));
        let Err(SetupError::Failed(why)) = sids.sid_for(&matter()) else {
            panic!("sid_for went ahead while the lock was held");
        };
        assert!(why.contains(MATTERS_LOCK), "{why}");
        assert!(!why.contains(&*folder.to_string_lossy()), "{why}");
        assert!(sids.is_empty());
    }
}
