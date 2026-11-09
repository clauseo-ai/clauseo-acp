//! Turning WER off through a fake registry: the pool hive opened by SID part by part, the values
//! set and read back, each failure refusing (a hive not loaded, a link on the way, an unsafe key,
//! a failed open, write or read, a value that reads back wrong), a WER policy logged, and
//! set-up's repair deleting a link or an unsafe key. The security checks read fake descriptors.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::super::super::acl_bytes::{AceSpec, DENIED, build_descriptor, empty_acl, sid_bytes};
use super::*;

const POOL: &str = "S-1-5-21-1-2-3-1001";
const KEY: &str = r"S-1-5-21-1-2-3-1001\Software\Microsoft\Windows\Windows Error Reporting";

fn pool() -> Sid {
    Sid::parse(POOL).unwrap()
}

/// The pool user's own profile, not temporary.
fn home() -> Profile {
    Profile {
        state: Some(0),
        path: PathBuf::from(r"C:\Users\ClauseoSb01"),
    }
}

fn ace(kind: u8, flags: u8, mask: u32, sid: &str) -> AceSpec {
    AceSpec {
        kind,
        flags,
        mask,
        sid: sid_bytes(&Sid::parse(sid).unwrap()),
    }
}

/// A descriptor with `dacl`'s entries and, when given, a label entry at `level` with `mask`.
fn descriptor(dacl: &[AceSpec], label: Option<(u32, u32)>) -> Vec<u8> {
    let mut acl = empty_acl();
    for spec in dacl {
        acl = Acl::parse(&acl).unwrap().with(spec);
    }
    let sacl = label.map(|(level, mask)| {
        Acl::parse(&empty_acl())
            .unwrap()
            .with(&ace(LABEL, 0, mask, &format!("S-1-16-{level}")))
    });
    build_descriptor(0, Some(&acl), sacl.as_deref())
}

/// The default: the pool user, `SYSTEM` and the administrators with full control, `RESTRICTED`
/// with read, no explicit label.
fn safe() -> Vec<u8> {
    descriptor(
        &[
            ace(ALLOWED, 0, 0x000F_003F, POOL),
            ace(ALLOWED, 0, 0x000F_003F, "S-1-5-18"),
            ace(ALLOWED, 0, 0x000F_003F, "S-1-5-32-544"),
            ace(ALLOWED, 0, 0x0002_0019, "S-1-5-12"),
        ],
        None,
    )
}

#[derive(Default, Clone)]
struct Node {
    link: bool,
    /// Its DACL shuts the administrators out: a delete open is refused (5) until taken.
    deny_admins: bool,
    values: BTreeMap<String, u32>,
    security: Vec<u8>,
}

/// Keys by full path under `HKEY_USERS`; fails the calls it is told to.
#[derive(Default)]
struct Fake {
    keys: BTreeMap<String, Node>,
    machine_policy: Option<u32>,
    fail_write: Option<&'static str>,
    fail_read: Option<&'static str>,
    /// Writes of this value are dropped.
    drop_write: Option<&'static str>,
    /// Plain reads (as WER reads) of this value find nothing.
    plain_missing: Option<&'static str>,
    deleted: Vec<String>,
    taken: Vec<String>,
}

impl Fake {
    /// A loaded pool hive.
    fn loaded() -> Fake {
        let mut fake = Fake::default();
        fake.keys.insert(POOL.to_owned(), Node::default());
        fake
    }
}

impl Registry for Fake {
    type Key = String;
    fn open(
        &mut self,
        parent: Option<&String>,
        name: &str,
        how: How,
    ) -> Result<Option<String>, u32> {
        let path = parent.map_or_else(|| name.to_owned(), |p| format!("{p}\\{name}"));
        if !self.keys.contains_key(&path) {
            if how != How::Make {
                return Ok(None);
            }
            let security = safe();
            self.keys.insert(
                path.clone(),
                Node {
                    security,
                    ..Node::default()
                },
            );
        }
        if how == How::Delete && self.keys[&path].deny_admins {
            return Err(ACCESS_DENIED);
        }
        Ok(Some(path))
    }
    fn is_link(&mut self, key: &String) -> Result<bool, u32> {
        Ok(self.keys[key].link)
    }
    fn security(&mut self, key: &String) -> Result<Vec<u8>, u32> {
        Ok(self.keys[key].security.clone())
    }
    fn set_dword(&mut self, key: &String, name: &str, value: u32) -> Result<(), u32> {
        if self.fail_write == Some(name) {
            return Err(5);
        }
        if self.drop_write != Some(name) {
            self.keys
                .get_mut(key)
                .unwrap()
                .values
                .insert(name.to_owned(), value);
        }
        Ok(())
    }
    fn dword(&mut self, key: &String, name: &str) -> Result<Option<u32>, u32> {
        if self.fail_read == Some(name) {
            return Err(87);
        }
        Ok(self.keys[key].values.get(name).copied())
    }
    fn read(&mut self, root: Root, path: &str, name: &str) -> Result<Option<u32>, u32> {
        match root {
            Root::Machine => Ok(self.machine_policy.filter(|_| name == "Disabled")),
            Root::Users if self.plain_missing == Some(name) => Ok(None),
            Root::Users => Ok(self
                .keys
                .get(path)
                .and_then(|n| n.values.get(name).copied())),
        }
    }
    fn delete(&mut self, key: String) -> Result<(), u32> {
        self.keys
            .retain(|path, _| path != &key && !path.starts_with(&format!("{key}\\")));
        self.deleted.push(key);
        Ok(())
    }
    fn take(&mut self, parent: &String, name: &str) -> Result<(), u32> {
        let path = format!("{parent}\\{name}");
        let node = self.keys.get_mut(&path).ok_or(2u32)?;
        node.deny_admins = false;
        node.security = safe();
        self.taken.push(path);
        Ok(())
    }
}

const VALUES: [(&str, u32); 4] = [
    ("Disabled", 1),
    ("DontSendAdditionalData", 1),
    ("DontShowUI", 1),
    ("LoggingDisabled", 1),
];

fn values(fake: &Fake) -> Vec<(&str, u32)> {
    fake.keys[KEY]
        .values
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect()
}

#[test]
fn the_pool_hives_wer_key_gets_all_four_values() {
    let mut fake = Fake::loaded();
    assert_eq!(quiet(&mut fake, &pool(), &home()), Ok(vec![]));
    assert_eq!(values(&fake), VALUES);
}

#[test]
fn setting_the_values_again_changes_nothing() {
    let mut fake = Fake::loaded();
    quiet(&mut fake, &pool(), &home()).unwrap();
    quiet(&mut fake, &pool(), &home()).unwrap();
    assert_eq!(values(&fake), VALUES);
}

#[test]
fn a_hive_that_isnt_loaded_refuses_and_nothing_is_made() {
    let mut fake = Fake::default();
    assert_eq!(
        quiet(&mut fake, &pool(), &home()),
        Err(WerError::NotLoaded {
            sid: POOL.to_owned()
        })
    );
    assert!(fake.keys.is_empty());
}

#[test]
fn a_sid_that_isnt_a_users_never_opens_a_hive() {
    for sid in ["S-1-5-18", "S-1-1-0"] {
        let mut fake = Fake::loaded();
        assert_eq!(
            quiet(&mut fake, &Sid::parse(sid).unwrap(), &home()),
            Err(WerError::NotAUser {
                sid: sid.to_owned()
            })
        );
    }
}

#[test]
fn a_link_on_the_way_or_at_the_key_refuses_before_any_write() {
    for (at, part) in [
        ("Software\\Microsoft", "Microsoft"),
        (KEY, "Windows Error Reporting"),
    ] {
        let mut fake = Fake::loaded();
        let path = if at == KEY {
            KEY.to_owned()
        } else {
            format!("{POOL}\\{at}")
        };
        fake.keys.insert(
            format!("{POOL}\\Software"),
            Node {
                security: safe(),
                ..Node::default()
            },
        );
        fake.keys.insert(
            path,
            Node {
                link: true,
                security: safe(),
                ..Node::default()
            },
        );
        if at == KEY {
            for mid in ["Software\\Microsoft", "Software\\Microsoft\\Windows"] {
                fake.keys.insert(
                    format!("{POOL}\\{mid}"),
                    Node {
                        security: safe(),
                        ..Node::default()
                    },
                );
            }
        }
        assert_eq!(
            quiet(&mut fake, &pool(), &home()),
            Err(WerError::Link { part })
        );
        assert!(fake.keys.values().all(|n| n.values.is_empty()));
    }
}

#[test]
fn a_low_labelled_or_widely_writable_key_refuses() {
    let unsafe_keys = [
        (
            descriptor(&[ace(ALLOWED, 0, 0x000F_003F, POOL)], Some((0x1000, 1))),
            "labelled below Medium",
        ),
        (
            descriptor(&[ace(ALLOWED, 0, 0x000F_003F, POOL)], Some((0x2000, 0))),
            "doesn't stop writes",
        ),
        (
            descriptor(&[ace(ALLOWED, 0, 0x0002, "S-1-5-12")], None),
            "written by S-1-5-12",
        ),
        (
            descriptor(&[ace(ALLOWED, 0, 0x1000_0000, "S-1-1-0")], None),
            "written by S-1-1-0",
        ),
        (
            descriptor(&[ace(ALLOWED, 0, 0x0001_0000, "S-1-5-32-545")], None),
            "written by S-1-5-32-545",
        ),
        (build_descriptor(0, None, None), "no DACL"),
    ];
    for (security, why) in unsafe_keys {
        let fault = security_fault(&security, &pool());
        assert!(
            fault.as_deref().is_some_and(|f| f.contains(why)),
            "{why}: {fault:?}"
        );
        let mut fake = Fake::loaded();
        quiet(&mut fake, &pool(), &home()).unwrap();
        fake.keys.get_mut(KEY).unwrap().security = security;
        fake.keys.get_mut(KEY).unwrap().values.clear();
        assert!(
            matches!(
                quiet(&mut fake, &pool(), &home()),
                Err(WerError::Unsafe { .. })
            ),
            "{why}"
        );
        assert!(fake.keys[KEY].values.is_empty(), "{why}");
    }
}

#[test]
fn a_medium_label_reads_rights_inherit_only_and_deny_entries_pass() {
    let ok = [
        safe(),
        descriptor(&[ace(ALLOWED, 0, 0x000F_003F, POOL)], Some((0x2000, 1))),
        descriptor(&[ace(ALLOWED, 0, 0x000F_003F, POOL)], Some((0x3000, 1))),
        descriptor(&[ace(ALLOWED, INHERIT_ONLY, 0x1000_0000, "S-1-3-0")], None),
        descriptor(&[ace(DENIED, 0, 0x000F_003F, "S-1-1-0")], None),
        descriptor(&[ace(ALLOWED, 0, 0x0002_0019, "S-1-15-2-1")], None),
    ];
    for security in ok {
        assert_eq!(security_fault(&security, &pool()), None);
    }
}

#[test]
fn a_failed_write_is_refused() {
    let mut fake = Fake {
        fail_write: Some("DontShowUI"),
        ..Fake::loaded()
    };
    assert_eq!(
        quiet(&mut fake, &pool(), &home()),
        Err(WerError::Write {
            name: "DontShowUI",
            code: 5
        })
    );
}

#[test]
fn a_failed_read_back_is_refused() {
    let mut fake = Fake {
        fail_read: Some("LoggingDisabled"),
        ..Fake::loaded()
    };
    assert_eq!(
        quiet(&mut fake, &pool(), &home()),
        Err(WerError::Read {
            name: "LoggingDisabled",
            code: 87
        })
    );
}

#[test]
fn a_value_that_reads_back_wrong_or_missing_is_refused() {
    let mut fake = Fake {
        drop_write: Some("Disabled"),
        ..Fake::loaded()
    };
    assert_eq!(
        quiet(&mut fake, &pool(), &home()),
        Err(WerError::ReadBack {
            name: "Disabled",
            found: None
        })
    );
    let mut plain = Fake {
        plain_missing: Some("DontSendAdditionalData"),
        ..Fake::loaded()
    };
    assert_eq!(
        quiet(&mut plain, &pool(), &home()),
        Err(WerError::ReadBack {
            name: "DontSendAdditionalData",
            found: None
        })
    );
}

#[test]
fn a_policy_keeping_wer_on_is_logged_and_the_launch_goes_on() {
    let mut fake = Fake {
        machine_policy: Some(0),
        ..Fake::loaded()
    };
    let lines = quiet(&mut fake, &pool(), &home()).unwrap();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("a machine policy (HKLM) keeps Windows Error Reporting on"));
    assert_eq!(values(&fake), VALUES);
    let mut user = Fake::loaded();
    let policy = format!("{POOL}\\{WER_POLICY}");
    let mut node = Node::default();
    node.values.insert("Disabled".to_owned(), 0);
    user.keys.insert(policy, node);
    let lines = quiet(&mut user, &pool(), &home()).unwrap();
    assert!(lines.len() == 1 && lines[0].contains("a policy for the sandbox user"));
    let mut off = Fake {
        machine_policy: Some(1),
        ..Fake::loaded()
    };
    assert_eq!(quiet(&mut off, &pool(), &home()), Ok(vec![]));
}

#[test]
fn set_ups_repair_deletes_a_link_on_the_way_or_at_the_key() {
    let mut fake = Fake::loaded();
    quiet(&mut fake, &pool(), &home()).unwrap();
    fake.keys.get_mut(KEY).unwrap().link = true;
    assert_eq!(repair(&mut fake, &pool()), Ok(true));
    assert_eq!(fake.deleted, [KEY]);
    assert_eq!(quiet(&mut fake, &pool(), &home()), Ok(vec![]));
    let windows = format!("{POOL}\\Software\\Microsoft\\Windows");
    fake.keys.get_mut(&windows).unwrap().link = true;
    assert_eq!(repair(&mut fake, &pool()), Ok(true));
    assert_eq!(fake.deleted.last(), Some(&windows));
    assert_eq!(quiet(&mut fake, &pool(), &home()), Ok(vec![]));
}

#[test]
fn set_ups_repair_deletes_an_unsafe_key_and_keeps_a_safe_one() {
    let mut fake = Fake::loaded();
    quiet(&mut fake, &pool(), &home()).unwrap();
    assert_eq!(repair(&mut fake, &pool()), Ok(false));
    assert_eq!(fake.deleted, Vec::<String>::new());
    fake.keys.get_mut(KEY).unwrap().security =
        descriptor(&[ace(ALLOWED, 0, 0x000F_003F, POOL)], Some((0x1000, 1)));
    assert_eq!(repair(&mut fake, &pool()), Ok(true));
    assert_eq!(fake.deleted, [KEY]);
    assert_eq!(quiet(&mut fake, &pool(), &home()), Ok(vec![]));
}

#[test]
fn set_ups_repair_of_a_hive_with_no_wer_key_does_nothing() {
    let mut fake = Fake::loaded();
    assert_eq!(repair(&mut fake, &pool()), Ok(false));
    assert_eq!(
        repair(&mut Fake::default(), &pool()),
        Err(WerError::NotLoaded {
            sid: POOL.to_owned()
        })
    );
}

#[test]
fn the_failure_names_the_sandbox_user_and_the_cause() {
    assert_eq!(
        WerError::Link {
            part: "Windows Error Reporting"
        }
        .to_string(),
        "turning off Windows Error Reporting for the sandbox user failed: Windows Error Reporting \
         is a registry link"
    );
}

#[test]
fn a_temporary_profile_refuses_before_any_write() {
    let temporary = [
        Profile {
            state: Some(0x0800),
            path: PathBuf::from(r"C:\Users\ClauseoSb01"),
        },
        Profile {
            state: Some(0),
            path: PathBuf::from(r"C:\Users\TEMP.PC.000"),
        },
    ];
    for profile in temporary {
        let mut fake = Fake::loaded();
        assert!(matches!(
            quiet(&mut fake, &pool(), &profile),
            Err(WerError::Temporary { .. })
        ));
        assert_eq!(fake.keys.len(), 1, "nothing made: {profile:?}");
    }
}

#[test]
fn set_ups_repair_takes_a_key_that_shuts_the_administrators_out_then_deletes_it() {
    let mut fake = Fake::loaded();
    quiet(&mut fake, &pool(), &home()).unwrap();
    let key = fake.keys.get_mut(KEY).unwrap();
    key.link = true;
    key.deny_admins = true;
    assert_eq!(repair(&mut fake, &pool()), Ok(true));
    assert_eq!(fake.taken, [KEY]);
    assert_eq!(fake.deleted, [KEY]);
    assert_eq!(quiet(&mut fake, &pool(), &home()), Ok(vec![]));
}
