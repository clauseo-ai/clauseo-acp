//! The paths the admin-only log names: only tasks that keep the run at 56, at most five, each
//! with its non-ASCII characters replaced, its SIDs redacted, its quotes replaced and its text
//! in quotes, and cut at 260. The world-readable line names none; the admin-only log's
//! descriptor and cap.

use super::file_owner::{Dacl, descriptor};
use super::*;
use crate::windows::admin_copy::level_safe;
use crate::windows::setup::sweep::log_path::{
    ADMIN_FOLDER_SDDL, ADMIN_LOG_CAP, ADMIN_LOG_FOLDERS, ADMIN_LOG_FOLDERS_AS_ADMIN,
    ADMIN_LOG_NAME, ADMIN_LOG_SDDL, ADMIN_LOG_SDDL_AS_ADMIN, CLAUSEO_FOLDER_SDDL, LOGGED_CAP,
    PATH_CAP, admin_only, clean, listed, rotates,
};

#[test]
fn a_sid_in_a_name_is_redacted() {
    assert_eq!(
        clean(r"\OneDrive Reporting Task-S-1-5-21-1-2-3-1001"),
        r#""\OneDrive Reporting Task-<sid>""#
    );
    // Either case, and only SID-shaped runs: S-1-5 alone, or S-1- with no digits, stay.
    assert_eq!(clean(r"\A\s-1-5-18x"), r#""\A\<sid>x""#);
    assert_eq!(clean(r"\A\S-1-5 and S-1-x"), r#""\A\S-1-5 and S-1-x""#);
}

#[test]
fn a_hex_authority_sid_is_redacted_in_either_case() {
    assert_eq!(clean(r"\A\S-1-0x5-21-1-2-3"), r#""\A\<sid>""#);
    assert_eq!(clean(r"\A\s-1-0X5-21-1-2-3 x"), r#""\A\<sid> x""#);
    // No hex digit, or no group after the authority, is not a SID.
    assert_eq!(clean(r"\A\S-1-0xZ-1"), r#""\A\S-1-0xZ-1""#);
    assert_eq!(clean(r"\A\S-1-0x5"), r#""\A\S-1-0x5""#);
}

#[test]
fn two_sids_are_each_redacted() {
    assert_eq!(
        clean(r"\S-1-5-21-9-9-9-500\Task S-1-12-1-2-3-4-5 done"),
        r#""\<sid>\Task <sid> done""#
    );
}

#[test]
fn control_bidi_and_invisible_characters_are_replaced() {
    let path = "\\A\\a\u{0}b\u{7}c\u{7F}d\u{85}e\u{200B}f\u{202E}g\u{2066}h\u{61C}i\u{FEFF}j";
    assert_eq!(clean(path), r#""\A\a?b?c?d?e?f?g?h?i?j""#);
}

#[test]
fn every_character_outside_printable_ascii_is_replaced() {
    // A line separator, a soft hyphen, a fullwidth digit, a look-alike hyphen, a word joiner,
    // a Hangul filler, a tag character and a variation selector.
    let path = "\\A\\x\u{2028}y\u{AD}z\u{FF11}w\u{2011}v\u{2060}u\u{3164}t\u{E0041}s\u{FE0F}";
    assert_eq!(clean(path), r#""\A\x?y?z?w?v?u?t?s?""#);
}

#[test]
fn a_sid_with_fullwidth_digits_leaks_no_digit() {
    let path = "\\A\\S-1-\u{FF15}-\u{FF12}\u{FF11}-\u{FF11}\u{FF10}\u{FF10}\u{FF11}";
    let cleaned = clean(path);
    // Each fullwidth digit is a `?`: the SID's digits don't survive, and no `<sid>` is made.
    assert_eq!(cleaned, r#""\A\S-1-?-??-????""#);
}

#[test]
fn a_quote_in_a_name_becomes_a_question_mark() {
    assert_eq!(clean(r#"\A\say "hi""#), r#""\A\say ?hi?""#);
}

#[test]
fn a_name_with_a_separator_stays_inside_one_quoted_path() {
    let path = r"\A\x, \Microsoft\Foo".to_owned();
    assert_eq!(
        listed(&[path]),
        r#"; tasks that keep the sweep at 56: "\A\x, \Microsoft\Foo""#
    );
}

#[test]
fn a_long_path_is_cut_at_260_characters_and_keeps_the_quote() {
    let cleaned = clean(&format!("\\{}", "a".repeat(299)));
    // The quote, 260 characters, the ellipsis and the closing quote.
    assert_eq!(cleaned.chars().count(), PATH_CAP + 3);
    assert!(cleaned.starts_with("\"\\"), "{cleaned}");
    assert!(cleaned.ends_with("a…\""), "{cleaned}");
    assert_eq!(
        clean(&"a".repeat(PATH_CAP)),
        format!("\"{}\"", "a".repeat(PATH_CAP))
    );
}

#[test]
fn at_most_five_paths_are_named_then_how_many_more() {
    let paths: Vec<String> = (0..8).map(|n| format!(r"\A\t{n}")).collect();
    let line = listed(&paths);
    assert_eq!(
        line,
        concat!(
            r#"; tasks that keep the sweep at 56: "\A\t0", "\A\t1", "\A\t2", "#,
            r#""\A\t3", "\A\t4" (+3 more)"#
        )
    );
    assert_eq!(LOGGED_CAP, 5);
    let twice = [r"\A\t".to_owned(), r"\A\t".to_owned()];
    assert_eq!(
        listed(&twice),
        r#"; tasks that keep the sweep at 56: "\A\t""#
    );
    assert_eq!(listed(&[]), "");
}

#[test]
fn only_a_task_that_keeps_the_run_at_56_is_named_and_only_in_the_admin_only_log() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "fine", user("S-1-5-18"));
    fake.task("\\A", "gone", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\B", "stuck", user("S-1-5-21-1-2-3-1010"));
    fake.delete_refuse = vec!["stuck".to_owned()];
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(
        fake.admin_logged,
        [r#"the sandbox's task sweep; tasks that keep the sweep at 56: "\B\stuck""#]
    );
    // The world-readable sink: every line, the count alone, no path or name.
    assert_eq!(fake.logged.len(), 1);
    for (_, line) in &fake.logged {
        assert!(line.ends_with("; 1 tasks keep it at 56, named in the admin-only log"));
        for named in ["stuck", r"\B", "fine", "gone", "keep the sweep"] {
            assert!(!line.contains(named), "{named}: {line}");
        }
    }
    // A run that ends 0 names none, and writes nothing to the admin-only log.
    let mut clean_run = Fake::new(&[1010]);
    clean_run.task("\\A", "fine", user("S-1-5-18"));
    assert_eq!(sweep(&mut clean_run), SweepExit::Done);
    assert!(!clean_run.logged[0].1.contains("keep"));
    assert_eq!(clean_run.admin_logged, Vec::<String>::new());
}

#[test]
fn an_admin_only_log_that_cant_be_written_still_names_no_path_in_the_world_readable_line() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\B", "stuck", user("S-1-5-21-1-2-3-1010"));
    fake.delete_refuse = vec!["stuck".to_owned()];
    fake.admin_fails = Some("CreateFileW failed (5)".to_owned());
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    let line = &fake.logged[0].1;
    let said = "; 1 tasks keep it at 56, not named: the admin-only log couldn't be written \
                (CreateFileW failed (5))";
    assert!(line.ends_with(said), "{line}");
    assert!(!line.contains("stuck") && !line.contains(r"\B"), "{line}");
}

#[test]
fn the_admin_only_log_is_made_for_system_and_administrators_alone() {
    assert_eq!(ADMIN_LOG_SDDL, "O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)");
    assert_eq!(ADMIN_FOLDER_SDDL, "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
    let folders = [
        ("Clauseo", CLAUSEO_FOLDER_SDDL),
        ("Sandbox", ADMIN_FOLDER_SDDL),
    ];
    assert_eq!(ADMIN_LOG_FOLDERS, folders);
    assert_eq!(ADMIN_LOG_NAME, "sweep-admin.log");
    // That descriptor, as bytes: owner SYSTEM, a protected DACL, full control for SYSTEM and
    // Administrators. Anything wider, unprotected or owned by another isn't admin-only.
    let protect = |mut bytes: Vec<u8>| {
        bytes[3] |= 0x10; // SE_DACL_PROTECTED (0x1000), the control word's high byte.
        bytes
    };
    let full = 0x001F_01FF;
    let ours: &[(u8, &str, u32)] = &[(0, "S-1-5-18", full), (0, "S-1-5-32-544", full)];
    assert!(admin_only(&protect(descriptor(
        Some("S-1-5-18"),
        Dacl::Of(ours)
    ))));
    assert!(!admin_only(&descriptor(Some("S-1-5-18"), Dacl::Of(ours))));
    let users: &[(u8, &str, u32)] = &[(0, "S-1-5-18", full), (0, "S-1-5-32-545", 0x0012_0089)];
    assert!(!admin_only(&protect(descriptor(
        Some("S-1-5-18"),
        Dacl::Of(users)
    ))));
    let pool = "S-1-5-21-1-2-3-1010";
    assert!(!admin_only(&protect(descriptor(
        Some(pool),
        Dacl::Of(ours)
    ))));
    assert!(!admin_only(&protect(descriptor(None, Dacl::Of(ours)))));
    assert!(!admin_only(&protect(descriptor(
        Some("S-1-5-18"),
        Dacl::Null
    ))));
    let deny: &[(u8, &str, u32)] = &[(0, "S-1-5-18", full), (1, "S-1-1-0", full)];
    assert!(!admin_only(&protect(descriptor(
        Some("S-1-5-18"),
        Dacl::Of(deny)
    ))));
}

#[test]
fn a_log_set_up_made_owned_by_administrators_passes_the_same_check() {
    assert_eq!(ADMIN_LOG_SDDL_AS_ADMIN, "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)");
    let folders = [
        ("Clauseo", CLAUSEO_FOLDER_SDDL),
        ("Sandbox", "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"),
    ];
    assert_eq!(ADMIN_LOG_FOLDERS_AS_ADMIN, folders);
    let protect = |mut bytes: Vec<u8>| {
        bytes[3] |= 0x10;
        bytes
    };
    let full = 0x001F_01FF;
    // ADMIN_LOG_SDDL_AS_ADMIN as bytes, its entries in either order: admin-only.
    let ours: &[(u8, &str, u32)] = &[(0, "S-1-5-18", full), (0, "S-1-5-32-544", full)];
    let swapped: &[(u8, &str, u32)] = &[(0, "S-1-5-32-544", full), (0, "S-1-5-18", full)];
    for dacl in [ours, swapped] {
        let made = protect(descriptor(Some("S-1-5-32-544"), Dacl::Of(dacl)));
        assert!(admin_only(&made));
    }
    // The DACL must be exactly those two, each full control: one alone, one with less, one
    // twice, or Users read beside them, isn't.
    let not = [
        vec![(0, "S-1-5-18", full)],
        vec![(0, "S-1-5-18", full), (0, "S-1-5-32-544", 0x0012_0089)],
        vec![(0, "S-1-5-18", full), (0, "S-1-5-18", full)],
        vec![
            (0, "S-1-5-18", full),
            (0, "S-1-5-32-544", full),
            (0, "S-1-5-32-545", 0x0012_0089),
        ],
    ];
    for dacl in &not {
        let made = protect(descriptor(Some("S-1-5-32-544"), Dacl::Of(dacl)));
        assert!(!admin_only(&made), "{dacl:?}");
    }
}

#[test]
fn the_clauseo_folder_is_made_as_the_installer_makes_it() {
    let hooks = concat!(env!("CARGO_MANIFEST_DIR"), "/../desktop/windows/hooks.nsh");
    let hooks = std::fs::read_to_string(hooks).unwrap();
    let made =
        format!("ConvertStringSecurityDescriptorToSecurityDescriptorW(w \"{CLAUSEO_FOLDER_SDDL}\"");
    assert!(
        hooks.contains(&made),
        "hooks.nsh no longer makes the folder with {made}"
    );
}

#[test]
fn an_installer_made_clauseo_folder_passes_and_a_user_owned_or_writable_one_doesnt() {
    let full = 0x001F_01FF;
    let users = "S-1-5-32-545";
    // The installer's: owner Administrators (or SYSTEM, when the sweep made it), Users read
    // and run.
    let installer: &[(u8, &str, u32)] = &[
        (0, "S-1-5-18", full),
        (0, "S-1-5-32-544", full),
        (0, users, 0x0012_00A9),
    ];
    for owner in ["S-1-5-32-544", "S-1-5-18"] {
        assert!(
            level_safe(&descriptor(Some(owner), Dacl::Of(installer))),
            "{owner}"
        );
    }
    // Owned by a standard user (who may always rewrite its DACL), or Users may write.
    let squatter = "S-1-5-21-1-2-3-1010";
    assert!(!level_safe(&descriptor(
        Some(squatter),
        Dacl::Of(installer)
    )));
    let writable: &[(u8, &str, u32)] = &[(0, "S-1-5-18", full), (0, users, 0x0013_01BF)];
    assert!(!level_safe(&descriptor(
        Some("S-1-5-32-544"),
        Dacl::Of(writable)
    )));
    assert!(!level_safe(&descriptor(Some("S-1-5-32-544"), Dacl::Null)));
}

#[test]
fn the_admin_only_log_rotates_once_past_256_kib() {
    assert_eq!(ADMIN_LOG_CAP, 256 * 1024);
    assert!(
        !rotates(0, ADMIN_LOG_CAP + 1),
        "a new log takes any first line"
    );
    assert!(!rotates(ADMIN_LOG_CAP - 10, 10));
    assert!(rotates(ADMIN_LOG_CAP - 10, 11));
    assert!(rotates(u64::MAX, 1));
}
