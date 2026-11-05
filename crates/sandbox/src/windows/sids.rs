//! Matter SIDs: one random SID per matter, the restricting SID that keeps one matter's agent
//! out of another's folders (S15, "The token"). Each is `S-1-5-21-<a>-<b>-<c>-<rid>` from the OS's
//! randomness, made at the matter's first launch and kept in `<data_dir>\sandbox\matters.json`
//! (matter id to SID). A matter keeps its SID for good: nothing here removes one, and only
//! `forget_matter` (S15 PR3, S15a) will. A SID is never handed to a second matter.
//!
//! Built on every OS so its tests run everywhere; only Windows uses it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

use super::write;
use crate::SetupError;

/// The folder under the data folder that holds the sandbox's records.
pub const SANDBOX_DIR: &str = "sandbox";
/// The matter SIDs' file, in [`SANDBOX_DIR`].
pub const MATTERS_FILE: &str = "matters.json";
/// The version `matters.json` is written at; a file at another version is refused, not
/// rewritten.
const MATTERS_VERSION: u32 = 1;
/// The fewest sub-authorities a SID may have here, and the most (`SID_MAX_SUB_AUTHORITIES`).
const SUB_AUTHORITIES: std::ops::RangeInclusive<usize> = 1..=15;
/// The largest identifier authority: 48 bits.
const AUTHORITY_MAX: u64 = (1 << 48) - 1;
/// A matter SID's relative id is at least this, clear of the well-known RIDs (500 and up).
const MATTER_RID_MIN: u32 = 1000;

/// A SID in its canonical string form: `S-1-<authority>-<sub>…`, decimal, no leading zeros, one
/// to fifteen sub-authorities, each a `u32`. Canonical, so two `Sid`s are equal exactly when they
/// name the same SID. `ConvertSidToStringSidW` writes this form; aliases such as `BA` and the
/// hex authority form are refused.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sid(String);

/// A string that is not a SID in [`Sid`]'s form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a SID: {0:?}")]
pub struct NotASid(pub String);

impl Sid {
    /// Checks `text` against [`Sid`]'s form.
    pub fn parse(text: &str) -> Result<Sid, NotASid> {
        let refuse = || NotASid(text.chars().take(200).collect());
        let rest = text.strip_prefix("S-1-").ok_or_else(refuse)?;
        let mut parts = rest.split('-');
        let authority = parts.next().and_then(decimal).ok_or_else(refuse)?;
        if authority > AUTHORITY_MAX {
            return Err(refuse());
        }
        let mut count = 0usize;
        for part in parts {
            let sub = decimal(part).ok_or_else(refuse)?;
            u32::try_from(sub).map_err(|_too_big| refuse())?;
            count += 1;
        }
        if !SUB_AUTHORITIES.contains(&count) {
            return Err(refuse());
        }
        Ok(Sid(text.to_owned()))
    }

    /// The SID's string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is a matter SID's shape: `S-1-5-21-<a>-<b>-<c>-<rid>`, `rid` at least 1000.
    pub fn is_matter_form(&self) -> bool {
        let subs: Vec<&str> = self.0.split('-').skip(3).collect();
        let rid = subs.last().and_then(|rid| rid.parse::<u32>().ok());
        self.0.starts_with("S-1-5-21-")
            && subs.len() == 5
            && rid.is_some_and(|rid| rid >= MATTER_RID_MIN)
    }

    /// A new matter SID from the OS's randomness: three random sub-authorities after `21`, and a
    /// random relative id of at least 1000.
    pub(crate) fn random_matter() -> io::Result<Sid> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        let [a, b, c, r] = [0usize, 4, 8, 12].map(|at| {
            let mut word = [0u8; 4];
            word.copy_from_slice(bytes.get(at..at + 4).unwrap_or(&[0; 4]));
            u32::from_le_bytes(word)
        });
        let rid = MATTER_RID_MIN + r % (u32::MAX - MATTER_RID_MIN + 1);
        Ok(Sid(format!("S-1-5-21-{a}-{b}-{c}-{rid}")))
    }
}

/// A canonical decimal: digits only, no sign, no leading zero unless it is `0`, at most 20 digits.
fn decimal(part: &str) -> Option<u64> {
    let canonical = !part.is_empty()
        && part.len() <= 20
        && part.bytes().all(|byte| byte.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'));
    if canonical { part.parse().ok() } else { None }
}

impl fmt::Display for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Sid {
    type Error = NotASid;

    fn try_from(text: String) -> Result<Sid, NotASid> {
        Sid::parse(&text)
    }
}

impl From<Sid> for String {
    fn from(sid: Sid) -> String {
        sid.0
    }
}

/// `matters.json` as it is on disk.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MattersFile {
    version: u32,
    matters: BTreeMap<MatterId, Sid>,
}

/// The matter SIDs of one data folder, as `matters.json` held them when loaded.
#[derive(Debug)]
pub struct MatterSids {
    path: PathBuf,
    sids: BTreeMap<MatterId, Sid>,
}

/// One load, change and save of `matters.json` at a time in this process, so two launches can't
/// each add a matter and lose the other's.
static MATTERS_LOCK: Mutex<()> = Mutex::new(());

/// The SID of `matter` in `data_dir`'s `matters.json`, made and saved first if the matter has
/// none. Loads, adds and saves under one process-wide lock.
pub fn matter_sid(data_dir: &Path, matter: &MatterId) -> Result<Sid, SetupError> {
    let _held = MATTERS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    MatterSids::load(data_dir)?.sid_for(matter)
}

impl MatterSids {
    /// Loads `<data_dir>\sandbox\matters.json`; no file is no matters. A file that can't be read
    /// or parsed, is at another version, holds a SID not of the matter form or holds one SID
    /// twice is an error, and is left as it is: rewriting it would lose matters' SIDs, and with
    /// them the way to remove their permission entries.
    pub fn load(data_dir: &Path) -> Result<MatterSids, SetupError> {
        let path = data_dir.join(SANDBOX_DIR).join(MATTERS_FILE);
        let failed = |why: String| SetupError::Failed(format!("{}: {why}", path.display()));
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(MatterSids {
                    path,
                    sids: BTreeMap::new(),
                });
            }
            Err(error) => return Err(failed(error.to_string())),
        };
        let file: MattersFile =
            serde_json::from_slice(&bytes).map_err(|error| failed(error.to_string()))?;
        if file.version != MATTERS_VERSION {
            return Err(failed(format!(
                "version {}, not {MATTERS_VERSION}",
                file.version
            )));
        }
        let mut seen = BTreeSet::new();
        for (matter, sid) in &file.matters {
            if !sid.is_matter_form() {
                return Err(failed(format!("{matter}'s SID {sid} is not a matter SID")));
            }
            if !seen.insert(sid) {
                return Err(failed(format!("{sid} is held by two matters")));
            }
        }
        Ok(MatterSids {
            path,
            sids: file.matters,
        })
    }

    /// The SID `matter` has, if any.
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

    /// The SID of `matter`. A matter without one gets a new random SID that no matter in the
    /// file holds, and the file is saved with it before it is returned; if the save fails, the
    /// matter has no SID and `self` is unchanged.
    pub fn sid_for(&mut self, matter: &MatterId) -> Result<Sid, SetupError> {
        if let Some(sid) = self.sids.get(matter) {
            return Ok(sid.clone());
        }
        let failed = |why: io::Error| SetupError::Failed(format!("matter SID: {why}"));
        let sid = loop {
            let sid = Sid::random_matter().map_err(failed)?;
            if !self.sids.values().any(|held| *held == sid) {
                break sid;
            }
        };
        let mut sids = self.sids.clone();
        sids.insert(*matter, sid.clone());
        self.save(&sids)?;
        self.sids = sids;
        Ok(sid)
    }

    /// Writes `sids` to the file atomically, making the sandbox folder if missing.
    fn save(&self, sids: &BTreeMap<MatterId, Sid>) -> Result<(), SetupError> {
        let failed = |why: String| SetupError::Failed(format!("{}: {why}", self.path.display()));
        let file = MattersFile {
            version: MATTERS_VERSION,
            matters: sids.clone(),
        };
        let mut bytes =
            serde_json::to_vec_pretty(&file).map_err(|error| failed(error.to_string()))?;
        bytes.push(b'\n');
        if let Some(folder) = self.path.parent() {
            write::make_dirs(folder).map_err(|error| failed(error.to_string()))?;
        }
        write::write_atomic(&self.path, &bytes).map_err(|error| failed(error.to_string()))
    }
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
    fn sids_in_the_canonical_form_parse() {
        for good in [
            "S-1-5-18",
            "S-1-5-32-544",
            "S-1-0-0",
            "S-1-5-21-2626480971-3383066999-180375651-600177987",
            "S-1-15-2-1-2-3-4-5-6-7",
            "S-1-281474976710655-4294967295",
        ] {
            assert_eq!(Sid::parse(good).unwrap().as_str(), good);
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in [
            "",
            "BA",
            "WD",
            "S-1",
            "S-1-5",
            "S-1-5-",
            "S-2-5-18",
            "s-1-5-18",
            "S-1-05-18",
            "S-1-5-018",
            "S-1-5-+18",
            "S-1-5--18",
            "S-1-0x5-18",
            "S-1-5-18 ",
            " S-1-5-18",
            "S-1-5-4294967296",
            "S-1-281474976710656-1",
            "S-1-5-1-2-3-4-5-6-7-8-9-10-11-12-13-14-15-16",
            "S-1-5-21-1-2-3-१२३४",
        ] {
            assert!(Sid::parse(bad).is_err(), "{bad:?}");
        }
        let json: Result<Sid, _> = serde_json::from_str("\"S-1-5-021\"");
        assert!(json.is_err());
    }

    #[test]
    fn a_new_matter_sid_has_the_matter_form_and_differs_each_time() {
        let mut seen = BTreeSet::new();
        for _ in 0..200 {
            let sid = Sid::random_matter().unwrap();
            assert!(sid.is_matter_form(), "{sid}");
            assert_eq!(Sid::parse(sid.as_str()).unwrap(), sid);
            assert!(seen.insert(sid.clone()), "{sid} came twice");
        }
        assert!(!Sid::parse("S-1-5-21-1-2-3-999").unwrap().is_matter_form());
        assert!(!Sid::parse("S-1-5-32-544").unwrap().is_matter_form());
        assert!(
            !Sid::parse("S-1-5-21-1-2-3-4-1000")
                .unwrap()
                .is_matter_form()
        );
        assert!(!Sid::parse("S-1-16-21-1-2-3-1000").unwrap().is_matter_form());
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
        let names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [MATTERS_FILE], "no temporary file is left");
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
}
