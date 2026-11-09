//! The removal through a fake: each kind listed and deleted, one failing kind leaving the rest,
//! counts alone in the log, and no kind reached over COM or naming tasks or BITS.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

/// Items by kind, each a name the log must never carry.
#[derive(Default)]
struct Fake {
    items: BTreeMap<&'static str, Vec<&'static str>>,
    /// A kind whose list fails.
    deny_list: BTreeSet<&'static str>,
    /// Items whose delete fails.
    deny_delete: BTreeSet<&'static str>,
    deleted: Vec<&'static str>,
    /// How many times the classes were listed.
    class_lists: usize,
}

fn key(kind: Kind) -> &'static str {
    match kind {
        Kind::Classes => "classes",
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
        self.deleted.push(item);
        Ok(())
    }
}

fn full() -> Fake {
    let mut fake = Fake::default();
    fake.items
        .insert("classes", vec!["CLSID", "Wow6432Node\\CLSID"]);
    fake.items
        .insert("run", vec!["Run\\backdoor", "RunOnce\\once"]);
    fake.items
        .insert("startup", vec!["evil.lnk", "subdir", "x.bat"]);
    fake
}

fn run(fake: &mut Fake) -> (bool, Vec<String>) {
    run_kinds(fake, &Kind::LOCAL)
}

fn run_kinds(fake: &mut Fake, kinds: &[Kind]) -> (bool, Vec<String>) {
    let mut lines = Vec::new();
    let clean = remove_all(fake, kinds, "before launch", &mut |_kind, detail| {
        lines.push(detail.text().to_owned());
    });
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
            "CLSID",
            "Wow6432Node\\CLSID",
            "Run\\backdoor",
            "RunOnce\\once",
            "evil.lnk",
            "subdir",
            "x.bat",
        ]
    );
    assert_eq!(
        lines,
        [
            "clauseo-sandbox-run: cleanup (before launch): removed 2 per-user COM class keys",
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
    fake.deny_list.insert("run");
    fake.deny_delete.insert("evil.lnk");
    let (clean, lines) = run(&mut fake);
    assert!(!clean);
    // The classes and the other Startup entries are still deleted.
    assert_eq!(
        fake.deleted,
        ["CLSID", "Wow6432Node\\CLSID", "subdir", "x.bat"]
    );
    assert_eq!(
        lines,
        [
            "clauseo-sandbox-run: cleanup (before launch): removed 2 per-user COM class keys",
            "clauseo-sandbox-run: cleanup (before launch): the Run and RunOnce values can't be \
             listed (1058)",
            "clauseo-sandbox-run: cleanup (before launch): removed 2 Startup folder entries, 1 \
             couldn't be (5)",
        ]
    );
    for line in &lines {
        for name in ["CLSID", "backdoor", "once", "evil", "subdir", "x.bat"] {
            assert!(!line.contains(name), "{line} names {name}");
        }
    }
}

#[test]
fn the_classes_go_first_and_a_failed_class_removal_leaves_the_rest() {
    assert_eq!(Kind::LOCAL.first(), Some(&Kind::Classes));
    let mut fake = full();
    fake.deny_delete.insert("CLSID");
    let (clean, _) = run(&mut fake);
    assert!(!clean);
    assert!(
        fake.deleted.contains(&"Run\\backdoor"),
        "the rest still run"
    );
    assert!(fake.deleted.contains(&"x.bat"), "the rest still run");
    let mut fake = full();
    fake.deny_list.insert("classes");
    let (clean, _) = run(&mut fake);
    assert!(!clean);
    assert!(fake.deleted.contains(&"x.bat"));
}

#[test]
fn the_classes_are_listed_once_and_no_task_or_bits_job_is_a_kind() {
    // The pool user's tasks and BITS jobs are the SYSTEM sweep task's: no kind here names them.
    let mut fake = full();
    assert!(run(&mut fake).0);
    assert_eq!(fake.class_lists, 1);
    assert_eq!(Kind::LOCAL, [Kind::Classes, Kind::RunKeys, Kind::Startup]);
    for kind in Kind::LOCAL {
        let name = kind.to_string();
        assert!(!name.contains("BITS") && !name.contains("task"), "{name}");
    }
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
