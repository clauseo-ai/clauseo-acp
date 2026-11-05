//! Claude's plans folder from a launch's environment, and which paths are a plan note in it, in
//! both path styles whatever the OS the tests run on.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::PermissionAbout;

use super::{about, is_plan_note, plans_folder};

fn env(pairs: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn the_plans_folder_is_in_claude_config_dir_else_in_the_home_claude_folder() {
    let both = env(&[
        ("CLAUDE_CONFIG_DIR", "/data/agents/claude-acp/home"),
        ("HOME", "/Users/lawyer"),
    ]);
    assert_eq!(
        plans_folder(&both, false),
        Some(PathBuf::from("/data/agents/claude-acp/home/plans"))
    );
    let empty = env(&[("CLAUDE_CONFIG_DIR", ""), ("HOME", "/Users/lawyer")]);
    assert_eq!(
        plans_folder(&empty, false),
        Some(PathBuf::from("/Users/lawyer/.claude/plans"))
    );
    assert_eq!(plans_folder(&env(&[("PATH", "/bin")]), false), None);
    // On Unix a name is matched as written, so another spelling is another variable.
    let lower = env(&[
        ("claude_config_dir", "/elsewhere"),
        ("HOME", "/Users/lawyer"),
    ]);
    assert_eq!(
        plans_folder(&lower, false),
        Some(PathBuf::from("/Users/lawyer/.claude/plans"))
    );
}

#[test]
fn on_windows_the_names_match_in_any_case_and_the_home_is_userprofile() {
    let set = env(&[("Claude_Config_Dir", r"C:\clauseo-g16\logins\claude")]);
    let plans = plans_folder(&set, true).unwrap();
    assert!(is_plan_note(
        &plans,
        Path::new(r"C:\clauseo-g16\logins\claude\plans\the-plan-has-one-splendid-turing.md"),
        true
    ));
    let home = env(&[("HOME", r"C:\msys"), ("UserProfile", r"C:\Users\Lawyer")]);
    let plans = plans_folder(&home, true).unwrap();
    assert!(is_plan_note(
        &plans,
        Path::new(r"C:\Users\Lawyer\.claude\plans\x.md"),
        true
    ));
}

#[test]
fn only_a_markdown_file_directly_in_the_plans_folder_is_a_plan_note() {
    let plans = Path::new("/home/lawyer/.claude/plans");
    let note = |path: &str| is_plan_note(plans, Path::new(path), false);
    assert!(note("/home/lawyer/.claude/plans/x.md"));
    assert!(note("/home/lawyer/.claude/./plans//x.md"));
    // The matter's own plans folder, nested, another extension, the config folder's own files.
    assert!(!note("/matters/lease/plans/x.md"));
    assert!(!note("/home/lawyer/.claude/plans/sub/x.md"));
    assert!(!note("/home/lawyer/.claude/plans/x.txt"));
    assert!(!note("/home/lawyer/.claude/plans/.md"));
    assert!(!note("/home/lawyer/.claude/plans"));
    assert!(!note("/home/lawyer/.claude/settings.json"));
    assert!(!note("/home/lawyer/.claude/settings.md"));
    // A `..` that escapes, and one that comes back, are both refused; so is a relative path.
    assert!(!note("/home/lawyer/.claude/plans/../settings.md"));
    assert!(!note("/home/lawyer/.claude/plans/../plans/x.md"));
    assert!(!note("home/lawyer/.claude/plans/x.md"));
    assert!(!note("plans/x.md"));
    // Unix compares names as written.
    assert!(!note("/home/lawyer/.Claude/plans/x.md"));
    assert!(!is_plan_note(
        Path::new(".claude/plans"),
        Path::new(".claude/plans/x.md"),
        false
    ));
}

#[test]
fn windows_paths_compare_in_any_case_with_either_separator_and_without_the_verbatim_prefix() {
    let plans = Path::new(r"C:\Users\Lawyer\.claude\plans");
    let note = |path: &str| is_plan_note(plans, Path::new(path), true);
    assert!(note(r"C:\Users\Lawyer\.claude\plans\x.md"));
    assert!(note(r"c:\users\LAWYER\.Claude\Plans\X.MD"));
    assert!(note("C:/Users/Lawyer/.claude/plans/x.md"));
    assert!(note(r"\\?\C:\Users\Lawyer\.claude\plans\x.md"));
    assert!(!note(r"D:\Users\Lawyer\.claude\plans\x.md"));
    assert!(!note(r"C:\Users\Lawyer\.claude\plans\sub\x.md"));
    assert!(!note(r"C:\Users\Lawyer\.claude\plans\x.txt"));
    assert!(!note(r"C:\Users\Lawyer\.claude\settings.json"));
    assert!(!note(r"C:\Users\Lawyer\.claude\plans\..\settings.md"));
    assert!(!note(r"C:Users\Lawyer\.claude\plans\x.md"));
    assert!(!note(r"\Users\Lawyer\.claude\plans\x.md"));
    assert!(!note(r"\\.\C:\Users\Lawyer\.claude\plans\x.md"));
    let share = Path::new(r"\\Server\Share\claude\plans");
    assert!(is_plan_note(
        share,
        Path::new(r"\\?\UNC\server\share\Claude\plans\x.md"),
        true
    ));
    assert!(!is_plan_note(
        share,
        Path::new(r"\\server\other\claude\plans\x.md"),
        true
    ));
}

/// Windows folds names one character at a time to upper case, not by Unicode's lowercase:
/// the Kelvin sign, the Ohm sign and `İ` are other names than `k`, `ω` and `i` on NTFS, so a
/// note in such a folder is no plan note of `kim`'s, `ωmega`'s or `iris`'s.
#[test]
fn windows_names_fold_as_ntfs_does_not_as_unicode_lowercase() {
    let note = |plans: &str, path: &str| is_plan_note(Path::new(plans), Path::new(path), true);
    assert!(!note(
        r"C:\Users\kim\.claude\plans",
        "C:\\Users\\\u{212A}im\\.claude\\plans\\x.md"
    ));
    assert!(!note(
        "C:\\Users\\\u{3c9}mega\\.claude\\plans",
        "C:\\Users\\\u{2126}mega\\.claude\\plans\\x.md"
    ));
    assert!(!note(
        r"C:\Users\iris\.claude\plans",
        "C:\\Users\\\u{130}ris\\.claude\\plans\\x.md"
    ));
    assert!(note(
        r"C:\Users\kim\.claude\plans",
        r"C:\USERS\KIM\.CLAUDE\PLANS\x.md"
    ));
}

#[test]
fn a_request_is_about_the_plan_only_from_claude_naming_one_plan_note() {
    let plans = Path::new("/home/lawyer/.claude/plans");
    let note = PathBuf::from("/home/lawyer/.claude/plans/x.md");
    let matter = PathBuf::from("/matters/lease/plans/x.md");
    assert_eq!(
        about(Some(plans), std::slice::from_ref(&note), false),
        Some(PermissionAbout::AgentPlan)
    );
    // Not Claude (no plans folder), no path, or more than one.
    assert_eq!(about(None, std::slice::from_ref(&note), false), None);
    assert_eq!(about(Some(plans), &[], false), None);
    assert_eq!(
        about(Some(plans), std::slice::from_ref(&matter), false),
        None
    );
    assert_eq!(about(Some(plans), &[note, matter], false), None);
}
