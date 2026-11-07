//! The record check before any `File` into the admin folder (`CLAUSEO_RECORD_CHECK`,
//! `admin_copy.nsh`), run on a fake registry: its lines are read from the source and run by
//! `nsis_run`, which fails on any line it doesn't know. No record
//! goes ahead (0); another owner is refused (1); a record holding an owner or version value that
//! can't be read as text is refused (3), as `--adopt` fails on it. Then both callers, the per-user
//! elevated instance and the per-machine installer, write the copy only when the check gives 0.

use super::admin_copy::{ASK, admin_code, assert_run, macro_body};
use super::nsis_run::{Machine, Node, Record, Value, chain, defines, lines, parse_all};

const ME: &str = "S-1-5-21-1-2-3-1001";
const OTHER: &str = "S-1-5-21-1-2-3-1002";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// `CLAUSEO_RECORD_CHECK OWNER` on `record`: its `$R1`, and whether it reached the version check.
fn check(owner: &str, record: Record) -> (i64, bool) {
    let code = admin_code();
    let mut defines = defines(&code);
    defines.insert("OWNER".to_owned(), owner.to_owned());
    let nodes = parse_all(&macro_body(&code, "CLAUSEO_RECORD_CHECK")[1..]);
    let mut machine = Machine::new(defines, record);
    machine.run(&nodes);
    let r1 = machine.vars.get("$R1").expect("$R1 set").parse().unwrap();
    let newer = machine.reached == ["CLAUSEO_NEWER_THAN_THIS"];
    assert!(newer || machine.reached.is_empty(), "{:?}", machine.reached);
    (r1, newer)
}

fn text(name: &'static str, value: &'static str) -> (&'static str, Value) {
    (name, Value::Text(value))
}

#[test]
fn no_record_goes_ahead() {
    for owner in [ME, ""] {
        // No key (the first install), or a key with no values.
        assert_eq!(check(owner, None), (0, false), "{owner}");
        assert_eq!(check(owner, Some(Vec::new())), (0, false), "{owner}");
    }
    // This user's own record, read whole.
    let own = vec![
        text("AdminCopySha256", SHA),
        text("AdminCopyOwner", ME),
        text("OwnerSid", ME),
        text("AdminCopyVersion", "0.1.0"),
    ];
    assert_eq!(check(ME, Some(own)), (0, true));
    // Per machine: the copy's values with no OwnerSid (nobody set up yet, or after a removal), or
    // a user's set-up beside a per-machine copy.
    let machine = vec![
        text("AdminCopySha256", SHA),
        text("AdminCopyOwner", "machine"),
        text("AdminCopyVersion", "0.1.0"),
    ];
    assert_eq!(check("", Some(machine.clone())), (0, true));
    let mut set_up = machine;
    set_up.push(text("OwnerSid", OTHER));
    assert_eq!(check("", Some(set_up)), (0, true));
    // A value the check doesn't read, unreadable: --adopt writes it again.
    assert_eq!(
        check(ME, Some(vec![("AdminCopySha256", Value::Unreadable)])),
        (0, false)
    );
}

#[test]
fn a_record_naming_another_owner_is_refused() {
    for record in [
        vec![text("AdminCopySha256", SHA), text("AdminCopyOwner", OTHER)],
        vec![
            text("AdminCopySha256", SHA),
            text("AdminCopyOwner", "machine"),
        ],
        vec![text("OwnerSid", OTHER)],
        // Compared exactly: a SID in another case isn't this one.
        vec![text("AdminCopyOwner", "s-1-5-21-1-2-3-1001")],
    ] {
        assert_eq!(check(ME, Some(record)), (1, false));
    }
    assert_eq!(
        check(
            "",
            Some(vec![
                text("AdminCopySha256", SHA),
                text("AdminCopyOwner", ME)
            ])
        ),
        (1, false)
    );
}

#[test]
fn a_record_whose_owner_or_version_cant_be_read_is_refused() {
    for owner in [ME, ""] {
        for name in ["AdminCopyOwner", "OwnerSid", "AdminCopyVersion", "ownersid"] {
            for value in [Value::Unreadable, Value::Text("")] {
                // The key's default value enumerates first, with an empty name; the walk goes on.
                let record = vec![
                    ("", Value::Unreadable),
                    text("AdminCopySha256", SHA),
                    (name, value),
                ];
                assert_eq!(check(owner, Some(record)), (3, false), "{owner} {name}");
            }
        }
        // Refused even beside a value naming this owner: the record is read whole first.
        let record = vec![text("AdminCopyOwner", ME), ("OwnerSid", Value::Unreadable)];
        assert_eq!(check(owner, Some(record)), (3, false), "{owner}");
    }
}

#[test]
fn both_callers_write_the_copy_only_when_the_check_gives_0() {
    let code = admin_code();
    for (name, refused) in [
        (
            "CLAUSEO_ELEVATED_BRANCH",
            vec!["IntOp $R8 ${CLAUSEO_ADMIN_EXIT_BASE} + ${CLAUSEO_ADOPT_FAILED}"],
        ),
        (
            "CLAUSEO_ADMIN_COPY_MACHINE",
            vec![
                "StrCpy $R8 ${CLAUSEO_ADOPT_FAILED}",
                "!insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS",
            ],
        ),
    ] {
        let body = &macro_body(&code, name)[1..];
        let nodes = parse_all(body);
        let Some(Node::If(arms, Some(otherwise))) = chain(&nodes, "If $R1 = 1") else {
            panic!("{name}: no ${{If}} $R1 = 1 chain with an ${{Else}}");
        };
        let conds: Vec<&[String]> = arms.iter().map(|(conds, _)| conds.as_slice()).collect();
        assert_eq!(
            conds,
            [["If $R1 = 1"], ["If $R1 = 2"], ["If $R1 = 3"]],
            "{name}"
        );
        // Nothing written in a refusal: no File, no copy.
        for (_, arm) in arms {
            assert!(
                lines(arm)
                    .iter()
                    .all(|line| !line.contains("CLAUSEO_ADMIN_COPY ")
                        && !line.contains("CLAUSEO_ADMIN_COPY_BOUNDED")
                        && !line.starts_with("File ")),
                "{name}"
            );
        }
        assert_eq!(lines(&arms[2].1), refused, "{name}");
        assert!(
            lines(otherwise)
                .iter()
                .any(|line| line.starts_with("!insertmacro CLAUSEO_ADMIN_COPY")),
            "{name}"
        );
    }
    // The outer per-user instance reads 100 + 3 as --adopt's failure: adminCopyFailed.
    let ask = macro_body(&code, ASK);
    assert_run(
        ask,
        &[
            "${Case4} ${CLAUSEO_ADOPT_USAGE} ${CLAUSEO_ADOPT_FAILED} ${CLAUSEO_ADOPT_LOCK_HELD} \
             ${CLAUSEO_ADOPT_DATA_DIR}",
            "StrCpy $R6 1",
        ],
    );
    assert_run(
        ask,
        &["${ElseIf} $R6 = 1", "DetailPrint \"$(adminCopyFailed)\""],
    );
}
