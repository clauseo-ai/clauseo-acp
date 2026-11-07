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
    /// Items whose delete fails with `NOT_LOGGED_ON`, as a pool user's BITS job's `Cancel`.
    not_logged_on: BTreeSet<&'static str>,
    deleted: Vec<&'static str>,
    /// Class keys that are back by the second class list: one registered again meanwhile.
    back: Vec<&'static str>,
    /// How many times the classes were listed.
    class_lists: usize,
}

fn key(kind: Kind) -> &'static str {
    match kind {
        Kind::Classes => "classes",
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
        if kind == Kind::Classes {
            self.class_lists += 1;
            if self.class_lists > 1 && !self.back.is_empty() {
                return Ok(self.back.clone());
            }
        }
        // What was deleted is gone.
        let items = self.items.get(key(kind)).cloned().unwrap_or_default();
        Ok(items
            .into_iter()
            .filter(|item| !self.deleted.contains(item))
            .collect())
    }
    fn delete(&mut self, _: Kind, item: &&'static str) -> io::Result<()> {
        if self.deny_delete.contains(item) {
            return Err(io::Error::from_raw_os_error(5));
        }
        if self.not_logged_on.contains(item) {
            return Err(io::Error::from_raw_os_error(NOT_LOGGED_ON));
        }
        self.deleted.push(item);
        Ok(())
    }
}

fn full() -> Fake {
    let mut fake = Fake::default();
    fake.items
        .insert("classes", vec!["CLSID", "Wow6432Node\\CLSID"]);
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
    run_kinds(fake, &Kind::ALL)
}

fn run_kinds(fake: &mut Fake, kinds: &[Kind]) -> (bool, Vec<String>) {
    let (removal, lines) = removal(fake, kinds);
    assert_eq!(removal.bits_for_stop, 0, "{lines:?}");
    (removal.clean, lines)
}

fn removal(fake: &mut Fake, kinds: &[Kind]) -> (Removal, Vec<String>) {
    let mut lines = Vec::new();
    let removal = remove_all(fake, kinds, "before launch", &mut |l| {
        lines.push(l.to_owned());
    });
    (removal, lines)
}

#[test]
fn each_kind_is_listed_and_every_item_deleted() {
    let mut fake = full();
    let (clean, lines) = run(&mut fake);
    assert!(clean);
    assert_eq!(
        fake.deleted,
        [
            "CLSID",
            "Wow6432Node\\CLSID",
            "Run\\backdoor",
            "RunOnce\\once",
            "evil.lnk",
            "subdir",
            "x.bat",
            "\\Evil\\AtLogon",
            "\\Updater",
            "{job-1}",
        ]
    );
    assert_eq!(
        lines,
        [
            "clauseo-sandbox-run: cleanup (before launch): removed 2 per-user COM class keys",
            "clauseo-sandbox-run: cleanup (before launch): removed 2 Run and RunOnce values",
            "clauseo-sandbox-run: cleanup (before launch): removed 3 Startup folder entries",
            "clauseo-sandbox-run: cleanup (before launch): removed 2 scheduled tasks",
            "clauseo-sandbox-run: cleanup (before launch): removed 1 BITS jobs",
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
            "CLSID",
            "Wow6432Node\\CLSID",
            "Run\\backdoor",
            "RunOnce\\once",
            "subdir",
            "x.bat",
            "\\Evil\\AtLogon",
            "\\Updater",
        ]
    );
    assert_eq!(
        lines,
        [
            "clauseo-sandbox-run: cleanup (before launch): removed 2 per-user COM class keys",
            "clauseo-sandbox-run: cleanup (before launch): removed 2 Run and RunOnce values",
            "clauseo-sandbox-run: cleanup (before launch): removed 2 Startup folder entries, 1 \
             couldn't be (5)",
            "clauseo-sandbox-run: cleanup (before launch): removed 2 scheduled tasks",
            "clauseo-sandbox-run: cleanup (before launch): the BITS jobs can't be listed (1058)",
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
fn a_bits_job_bits_cant_cancel_for_want_of_a_logon_is_counted_apart_and_no_failure() {
    let mut fake = full();
    fake.items
        .insert("bits", vec!["{job-1}", "{job-2}", "{job-3}"]);
    fake.not_logged_on.extend(["{job-1}", "{job-3}"]);
    let (removal, lines) = removal(&mut fake, &Kind::ALL);
    assert_eq!(
        removal,
        Removal {
            clean: true,
            bits_for_stop: 2
        }
    );
    assert!(fake.deleted.contains(&"{job-2}"));
    assert_eq!(
        lines.get(4..),
        Some(
            [
                "clauseo-sandbox-run: cleanup (before launch): 2 BITS jobs left for the system \
                 stop (1245): no logon token for them"
                    .to_owned(),
                "clauseo-sandbox-run: cleanup (before launch): removed 1 BITS jobs".to_owned(),
            ]
            .as_slice()
        )
    );
}

#[test]
fn any_other_bits_error_and_1245_elsewhere_stay_failures() {
    // Another error from a BITS job's `Cancel` fails the removal, the 1245 beside it counted.
    let mut fake = full();
    fake.items.insert("bits", vec!["{job-1}", "{job-2}"]);
    fake.not_logged_on.insert("{job-1}");
    fake.deny_delete.insert("{job-2}");
    let (left, lines) = removal(&mut fake, &Kind::ALL);
    assert_eq!(
        left,
        Removal {
            clean: false,
            bits_for_stop: 1
        }
    );
    assert!(
        lines
            .iter()
            .any(|l| l.ends_with("removed 0 BITS jobs, 1 couldn't be (5)")),
        "{lines:?}"
    );
    // 1245 from any other kind is a plain failure.
    let mut fake = full();
    fake.not_logged_on.insert("\\Updater");
    let (left, lines) = removal(&mut fake, &Kind::ALL);
    assert_eq!(
        left,
        Removal {
            clean: false,
            bits_for_stop: 0
        }
    );
    assert!(
        lines
            .iter()
            .any(|l| l.ends_with("removed 1 scheduled tasks, 1 couldn't be (1245)")),
        "{lines:?}"
    );
}

#[test]
fn the_inner_lists_nothing_over_com() {
    let mut fake = full();
    let (clean, lines) = run_kinds(&mut fake, &Kind::LOCAL);
    assert!(clean);
    assert!(Kind::LOCAL.iter().all(|kind| !kind.over_com()));
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        !fake.deleted.contains(&"{job-1}") && !fake.deleted.contains(&"\\Updater"),
        "{:?}",
        fake.deleted
    );
}

#[test]
fn the_classes_go_first_and_com_waits_for_them() {
    assert_eq!(Kind::ALL.first(), Some(&Kind::Classes));
    assert_eq!(Kind::LOCAL.first(), Some(&Kind::Classes));
    // A class key that can't be deleted keeps the end sweeper off COM.
    let mut fake = full();
    fake.deny_delete.insert("CLSID");
    let (clean, lines) = run(&mut fake);
    assert!(!clean);
    assert!(!fake.deleted.contains(&"\\Updater") && !fake.deleted.contains(&"{job-1}"));
    assert!(fake.deleted.contains(&"x.bat"), "the rest still run");
    let skipped = lines
        .iter()
        .filter(|l| l.contains("weren't listed"))
        .count();
    assert_eq!(skipped, 2, "{lines:?}");
    // So does a class list that fails.
    let mut fake = full();
    fake.deny_list.insert("classes");
    let (clean, _) = run(&mut fake);
    assert!(!clean);
    assert!(!fake.deleted.contains(&"{job-1}"));
}

#[test]
fn the_classes_are_looked_at_again_right_before_bits() {
    // All gone: listed once more right before BITS, which then runs.
    let mut fake = full();
    assert!(run(&mut fake).0);
    assert_eq!(fake.class_lists, 2);
    assert!(fake.deleted.contains(&"{job-1}"));
    // A class key registered again after the classes' removal: the tasks still go (their class
    // object comes from System32's module, with no lookup), BITS doesn't.
    let mut fake = full();
    fake.back = vec!["CLSID"];
    let (clean, lines) = run(&mut fake);
    assert!(!clean);
    assert!(fake.deleted.contains(&"\\Updater"));
    assert!(!fake.deleted.contains(&"{job-1}"));
    assert_eq!(
        lines.last().map(String::as_str),
        Some(
            "clauseo-sandbox-run: cleanup (before launch): the BITS jobs weren't listed: \
             per-user COM class keys are left"
        )
    );
    // A second class list that fails counts as a key left.
    let mut fake = full();
    fake.back = vec!["CLSID"];
    fake.deny_list.insert("classes");
    assert!(!run(&mut fake).0);
    assert!(!fake.deleted.contains(&"{job-1}"));
    // The inner, with no BITS, lists the classes once.
    let mut fake = full();
    assert!(run_kinds(&mut fake, &Kind::LOCAL).0);
    assert_eq!(fake.class_lists, 1);
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
