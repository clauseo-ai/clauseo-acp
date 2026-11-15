//! What Windows evaluates of the matter fence, pinned byte for byte (#1085's Windows run): for a
//! Windows launch with every kind of other matter (a sibling, a parent fenced by its siblings, an
//! invited one, one inside this matter), Claude's `Read` and `Edit` deny rules and allow rules in
//! Windows' form, the data folder's and the other matters' `Bash` and `PowerShell` rules (with
//! the variable forms and without), and which matter the auto-deny names for a set of shell
//! words. A change to any of them fails here; regenerate with `UPDATE_WINDOWS_RULES=1` only
//! when a Windows rule is meant to change (and say so: the VM rows run again).

use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::{LaunchDeny, claude_allow_rules_on, claude_deny_rules_on, matters};
use super::{windows_matter_shell_rules, windows_shell_deny_rules};
use crate::agent::matter_fence::{MatterFence, Other};

const MATTERS: &str = r"C:\Users\clauseo-lawyer\Matters";
/// `MATTERS` in its full 8.3 short form.
const SHORT_MATTERS: &str = r"C:\Users\CLAUSE~1\Matters";
const SNAP: &str = "src/agent/data_fence/windows_rules.snap";

fn id(n: u128) -> MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

fn env(name: &str) -> Option<OsString> {
    match name {
        "LOCALAPPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Local".into()),
        "APPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Roaming".into()),
        "USERPROFILE" | "HOME" => Some(r"C:\Users\clauseo-lawyer".into()),
        _ => None,
    }
}

/// The launch: this matter `Acme\Deal`, other matters `Smith` (sibling), `Acme` (parent, fenced
/// by `Acme\Old` and `Acme\notes.txt`), `Zed` (invited), `Acme\Deal\Child` (inside this one).
/// The profile folder has an 8.3 short name (`CLAUSE~1`), so `Smith`, this matter and the data
/// folder also go by their full short forms, as the launch adds them (`agent::short_forms`).
fn launch() -> LaunchDeny {
    let at = |name: &str| PathBuf::from(format!(r"{MATTERS}\{name}"));
    let others: [(&str, &[&str], bool); 4] = [
        ("Smith", &["Smith"], false),
        ("Acme", &[r"Acme\Old", r"Acme\notes.txt"], false),
        ("Zed", &["Zed"], true),
        (r"Acme\Deal\Child", &[r"Acme\Deal\Child"], false),
    ];
    let others = others
        .iter()
        .zip(1..)
        .map(|((name, paths, invited), n)| {
            let mut other = Other::new(id(n), (*name).to_owned(), at(name), *invited);
            other.spellings = vec![at(name)];
            other.paths = paths.iter().map(|path| at(path)).collect();
            if *name == "Smith" {
                let short = PathBuf::from(format!(r"{SHORT_MATTERS}\SMITH~1"));
                other.spellings.push(short.clone());
                other.paths.push(short);
            }
            other
        })
        .collect();
    let mut fence = MatterFence::asked(others);
    fence.own = vec![
        at(r"Acme\Deal"),
        PathBuf::from(format!(r"{SHORT_MATTERS}\Acme\Deal")),
    ];
    let mut deny = LaunchDeny::fixed(vec![
        PathBuf::from(r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop"),
        PathBuf::from(r"C:\Users\CLAUSE~1\AppData\Local\AICLAU~1.DES"),
    ]);
    deny.own_reads = vec!["run/s1/work".to_owned(), "agent-state/m1/notes".to_owned()];
    deny.matters = fence;
    deny
}

/// Every rule and decision, one per line, under a heading each.
fn rendered() -> String {
    let deny = launch();
    let mut out = String::new();
    let mut section = |title: &str, lines: Vec<String>| {
        out.push_str("## ");
        out.push_str(title);
        out.push('\n');
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
    };
    section("deny", claude_deny_rules_on(&deny, true));
    section("allow", claude_allow_rules_on(&deny, true));
    section("data shell", windows_shell_deny_rules(&deny, &env));
    for variables in [true, false] {
        let lists = windows_matter_shell_rules(&deny, &env, variables);
        let lines = lists
            .into_iter()
            .flat_map(|(matter, rules)| std::iter::once(format!("# {matter}")).chain(rules))
            .collect();
        section(&format!("matter shell, variables {variables}"), lines);
    }
    let fenced: Vec<(MatterId, Vec<PathBuf>)> = deny
        .matters
        .others
        .iter()
        .map(|other| (other.id, other.paths.clone()))
        .collect();
    let words = [
        r"C:\Users\clauseo-lawyer\Matters\Smith\lease.pdf",
        r"C:\Users\clauseo-lawyer\Matters\Smith 2\x",
        r"C:\Users\clauseo-lawyer\Matters\Acme\Old\memo.docx",
        r"C:\Users\clauseo-lawyer\Matters\Acme\Deal\memo.docx",
        r"C:\Users\clauseo-lawyer\Matters\Acme\Deal\Child\deed.pdf",
        r"C:\Users\clauseo-lawyer\Matters\Zed\a.txt",
        r"C:\Users\CLAUSE~1\Matters\SMITH~1\lease.pdf",
    ];
    let named = words
        .iter()
        .map(|word| {
            let found = matters::named_in_words(&[(*word).to_owned()], &fenced, true);
            format!("{word} -> {found:?}")
        })
        .collect();
    section("auto-deny words", named);
    out
}

#[test]
fn windows_rules_stay_byte_identical() {
    let now = rendered();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SNAP);
    if std::env::var_os("UPDATE_WINDOWS_RULES").is_some() {
        crate::write::write_file(&path, now.as_bytes()).unwrap();
        return;
    }
    let pinned = std::fs::read_to_string(&path).expect("the pinned Windows rules");
    assert!(pinned == now, "the Windows rules changed; see {SNAP}");
}
