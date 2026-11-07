//! The removal through a fake: each kind listed and deleted, one failing kind leaving the rest,
//! and counts alone in the log.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";

/// Items by kind, each a name the log must never carry.
#[derive(Default)]
struct Fake {
    items: BTreeMap<&'static str, Vec<&'static str>>,
    /// A kind whose list fails.
    deny_list: BTreeSet<&'static str>,
    /// Items whose delete fails.
    deny_delete: BTreeSet<&'static str>,
    deleted: Vec<&'static str>,
}

fn key(kind: Kind) -> &'static str {
    match kind {
        Kind::Tasks => "tasks",
        Kind::Bits => "bits",
        Kind::RunKeys => "run",
        Kind::Startup => "startup",
    }
}

impl Leftovers for Fake {
    type Item = &'static str;
    fn list(&mut self, kind: Kind) -> io::Result<Vec<&'static str>> {
        if self.deny_list.contains(key(kind)) {
            return Err(io::Error::from_raw_os_error(1058));
        }
        Ok(self.items.get(key(kind)).cloned().unwrap_or_default())
    }
    fn delete(&mut self, _: Kind, item: &&'static str) -> io::Result<()> {
        if self.deny_delete.contains(item) {
            return Err(io::Error::from_raw_os_error(5));
        }
        self.deleted.push(item);
        Ok(())
    }
}

fn full() -> Fake {
    let mut fake = Fake::default();
    fake.items
        .insert("tasks", vec!["\\Evil\\AtLogon", "\\Updater"]);
    fake.items.insert("bits", vec!["{job-1}"]);
    fake.items
        .insert("run", vec!["Run\\backdoor", "RunOnce\\once"]);
    fake.items
        .insert("startup", vec!["evil.lnk", "subdir", "x.bat"]);
    fake
}

fn run(fake: &mut Fake) -> (bool, Vec<String>) {
    let mut lines = Vec::new();
    let clean = remove_all(fake, "before launch", &mut |l| lines.push(l.to_owned()));
    (clean, lines)
}

#[test]
fn each_kind_is_listed_and_every_item_deleted() {
    let mut fake = full();
    let (clean, lines) = run(&mut fake);
    assert!(clean);
    assert_eq!(
        fake.deleted,
        [
            "\\Evil\\AtLogon",
            "\\Updater",
            "{job-1}",
            "Run\\backdoor",
            "RunOnce\\once",
            "evil.lnk",
            "subdir",
            "x.bat"
        ]
    );
    assert_eq!(
        lines,
        [
            "clauseo-sandbox-run: cleanup (before launch): removed 2 scheduled tasks",
            "clauseo-sandbox-run: cleanup (before launch): removed 1 BITS jobs",
            "clauseo-sandbox-run: cleanup (before launch): removed 2 Run and RunOnce values",
            "clauseo-sandbox-run: cleanup (before launch): removed 3 Startup folder entries",
        ]
    );
}

#[test]
fn nothing_left_is_clean_and_silent() {
    let mut fake = Fake::default();
    assert_eq!(run(&mut fake), (true, vec![]));
}

#[test]
fn one_kind_failing_leaves_the_others_and_the_log_names_no_item() {
    let mut fake = full();
    fake.deny_list.insert("bits");
    fake.deny_delete.insert("evil.lnk");
    let (clean, lines) = run(&mut fake);
    assert!(!clean);
    // The tasks before and the Run values and other Startup entries after are still deleted.
    assert_eq!(
        fake.deleted,
        [
            "\\Evil\\AtLogon",
            "\\Updater",
            "Run\\backdoor",
            "RunOnce\\once",
            "subdir",
            "x.bat"
        ]
    );
    assert_eq!(
        lines,
        [
            "clauseo-sandbox-run: cleanup (before launch): removed 2 scheduled tasks",
            "clauseo-sandbox-run: cleanup (before launch): the BITS jobs can't be listed (1058)",
            "clauseo-sandbox-run: cleanup (before launch): removed 2 Run and RunOnce values",
            "clauseo-sandbox-run: cleanup (before launch): removed 2 Startup folder entries, 1 \
             couldn't be (5)",
        ]
    );
    for line in &lines {
        for name in [
            "Evil", "Updater", "job-1", "backdoor", "once", "evil", "subdir", "x.bat",
        ] {
            assert!(!line.contains(name), "{line} names {name}");
        }
    }
}

#[test]
fn a_task_names_the_pool_user_by_sid_or_account() {
    let sid = Sid::parse(POOL).unwrap();
    for named in [
        POOL,
        "ClauseoSb03",
        "clauseosb03",
        "DESKTOP-1\\ClauseoSb03",
        ".\\ClauseoSb03",
        " ClauseoSb03 ",
    ] {
        assert!(names_user(named, &sid, "ClauseoSb03"), "{named}");
    }
    for named in [
        "",
        "ClauseoSb04",
        "DESKTOP-1\\Lawyer",
        "S-1-5-18",
        "SYSTEM",
        "ClauseoSb03x",
    ] {
        assert!(!names_user(named, &sid, "ClauseoSb03"), "{named}");
    }
    assert!(!names_user("", &sid, ""));
}

#[test]
fn the_startup_folder_must_lie_below_the_profile() {
    let profile = "C:\\Users\\ClauseoSb03";
    let startup = "C:\\Users\\ClauseoSb03\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup";
    assert_eq!(
        below(startup, profile),
        Some(vec![
            "AppData",
            "Roaming",
            "Microsoft",
            "Windows",
            "Start Menu",
            "Programs",
            "Startup"
        ])
    );
    assert_eq!(
        below(
            "c:\\users\\clauseosb03\\Startup",
            "C:\\Users\\ClauseoSb03\\"
        ),
        Some(vec!["Startup"])
    );
    for elsewhere in [
        "D:\\Matter",
        "C:\\Users\\ClauseoSb030\\Startup",
        "C:\\Users\\ClauseoSb03",
        "C:\\Users\\ClauseoSb03\\..\\Lawyer\\Startup",
        "C:\\Users\\ClauseoSb03\\a\\\\b",
        "C:\\Users\\ClauseoSb03\\.\\Startup",
        "C:\\Users\\ClauseoSb03\\a/../../x",
    ] {
        assert_eq!(below(elsewhere, profile), None, "{elsewhere}");
    }
    assert_eq!(below("C:\\x", ""), None);
}
