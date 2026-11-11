//! agent-browser's rules for Claude against the commands Claude writes, through a small model of
//! Claude Code's Bash rule matching (code.claude.com/docs/en/permissions, "Bash"):
//! - a command splits at `&&`, `||`, `;`, `|`, `&` and newlines outside quotes;
//! - a deny rule matching any part refuses the command, then an ask rule matching any part asks,
//!   then the command runs unasked only when every part matches an allow rule or is a `cd`
//!   (a `cd` into a working folder is read-only; the model takes every `cd` as one);
//! - `:*` at a rule's end means ` *`; `*` matches any text, spaces included; a trailing ` *`
//!   that is the rule's only `*` also matches the bare prefix.
//!
//! Not modelled: wrapper and env-assignment stripping, substitutions and control-flow bodies,
//! quote normalisation (rules see the command as written), redirects, read-only commands other
//! than `cd`, `auto` mode's classifier. What the model calls `Free` the mode decides: `acceptEdits`
//! asks, `auto`'s classifier reviews it.

use clauseo_profile::claude::{agent_browser_allowed, agent_browser_ask, agent_browser_denied};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Verdict {
    Deny,
    Ask,
    Allow,
    Free,
}

/// Whether `text` matches the glob `pattern`, whose `*` matches any text.
fn glob(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => {
            let Some(tail) = text.strip_prefix(head) else {
                return false;
            };
            (0..=tail.len())
                .filter(|&at| tail.is_char_boundary(at))
                .any(|at| glob(rest, &tail[at..]))
        }
    }
}

/// Whether the rule `Bash(<content>)` matches one part of a command.
fn rule_matches(rule: &str, part: &str) -> bool {
    let Some(content) = rule.strip_prefix("Bash(").and_then(|r| r.strip_suffix(')')) else {
        return false;
    };
    let content = match content.strip_suffix(":*") {
        Some(head) => format!("{head} *"),
        None => content.to_owned(),
    };
    if let Some(bare) = content.strip_suffix(" *")
        && !bare.contains('*')
        && part == bare
    {
        return true;
    }
    glob(&content, part)
}

/// The parts of `command`, split at the shell's separators outside quotes.
fn parts(command: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), _) if c == q => {
                quote = None;
                current.push(c);
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                current.push(c);
            }
            (None, '&' | '|' | ';' | '\n') => {
                if matches!(chars.peek(), Some('&' | '|')) {
                    chars.next();
                }
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect()
}

fn verdict(command: &str) -> Verdict {
    let (allow, ask, deny): (Vec<String>, Vec<String>, Vec<String>) = (
        agent_browser_allowed().collect(),
        agent_browser_ask().collect(),
        agent_browser_denied().collect(),
    );
    let parts = parts(command);
    let any = |rules: &[String]| {
        parts
            .iter()
            .any(|part| rules.iter().any(|rule| rule_matches(rule, part)))
    };
    if any(&deny) {
        Verdict::Deny
    } else if any(&ask) {
        Verdict::Ask
    } else if parts.iter().all(|part| {
        part == "cd" || part.starts_with("cd ") || allow.iter().any(|rule| rule_matches(rule, part))
    }) {
        Verdict::Allow
    } else {
        Verdict::Free
    }
}

fn assert_all(want: Verdict, commands: &[&str]) {
    for command in commands {
        assert_eq!(verdict(command), want, "{command}");
    }
}

#[test]
fn the_model_matches_the_documented_examples() {
    assert!(rule_matches("Bash(ls:*)", "ls"));
    assert!(rule_matches("Bash(ls *)", "ls -la"));
    assert!(!rule_matches("Bash(ls *)", "lsof"));
    assert!(rule_matches("Bash(ls*)", "lsof"));
    assert!(rule_matches("Bash(git * main)", "git push origin main"));
    assert!(!rule_matches("Bash(* --help *)", "npm --help"));
    assert_eq!(parts("a && b || c; d | e"), ["a", "b", "c", "d", "e"]);
    assert_eq!(parts("a \"x && y\" && b"), ["a \"x && y\"", "b"]);
}

/// Opening, reading and moving through pages never asks, in the forms Claude writes.
#[test]
fn browsing_and_reading_run_unasked() {
    assert_all(
        Verdict::Allow,
        &[
            "agent-browser open https://sci.gov.in",
            "agent-browser open \"https://www.sci.gov.in/judgements?a=1&b=2\"",
            "agent-browser open 'https://indiacode.nic.in/?q=a;b'",
            "agent-browser read https://example.com/article --json",
            "agent-browser read",
            "agent-browser snapshot",
            "agent-browser snapshot -i -u -c -d 3",
            "agent-browser snapshot -s \"#main\"",
            "agent-browser snapshot -i --json",
            "agent-browser --json snapshot -i",
            "agent-browser --json open https://sci.gov.in",
            "agent-browser --json get url",
            "agent-browser get text @e1",
            "agent-browser get attr @e10 href",
            "agent-browser get title",
            "agent-browser click @e1 --new-tab",
            "agent-browser find text \"Sign In\" click",
            "agent-browser fill @e2 \"lease deed\"",
            "agent-browser press Enter",
            "agent-browser scroll down 500",
            "agent-browser wait --text \"Judgment\"",
            "agent-browser wait --load domcontentloaded",
            "agent-browser wait --download",
            "agent-browser wait 2000",
            "agent-browser screenshot --full full.png",
            "agent-browser tab new https://sci.gov.in",
            "agent-browser tab t2",
            "agent-browser frame @e3",
            "agent-browser dialog accept",
            "agent-browser back",
            "agent-browser reload",
            "agent-browser close",
            "agent-browser open https://sci.gov.in && agent-browser wait --load load \
             && agent-browser snapshot -i",
            "agent-browser open https://sci.gov.in; agent-browser snapshot",
            "agent-browser open https://a.in\nagent-browser read",
            "cd /matter && agent-browser snapshot",
        ],
    );
}

/// `eval`, `pdf`, `download`, `upload`, a JavaScript wait, `cdp-url`, a local file and an
/// option that loads page JavaScript ask, bare, behind `--json` or chained.
#[test]
fn page_javascript_and_files_ask() {
    assert_all(
        Verdict::Ask,
        &[
            "agent-browser eval \"document.title\"",
            "agent-browser eval --stdin",
            "echo 1 | agent-browser eval --stdin",
            "agent-browser --json eval 1",
            "agent-browser open https://a.in && agent-browser eval 1",
            "agent-browser pdf page.pdf",
            "agent-browser download @e1 /tmp/x.pdf",
            "agent-browser upload @e5 a.pdf",
            "agent-browser wait --fn \"window.ok\"",
            "agent-browser wait -f x",
            "agent-browser --json wait --fn x",
            "agent-browser get cdp-url",
            "agent-browser --json get cdp-url",
            "agent-browser open https://a.in --init-script /tmp/a.js",
            "agent-browser --enable react-devtools open https://a.in",
            "agent-browser open file:///etc/passwd",
            "agent-browser tab new file:///Users/me",
            "agent-browser -p browserbase open https://a.in",
        ],
    );
}

/// The lawyer's browser and profile, Clauseo's session, config, state, downloads and proxy are
/// refused, wherever the option sits.
#[test]
fn the_denied_options_and_subcommands_are_refused() {
    assert_all(
        Verdict::Deny,
        &[
            "agent-browser --session x open https://a.in",
            "agent-browser open https://a.in --profile Default",
            "agent-browser --profile=Default open https://a.in",
            "agent-browser --headed open https://a.in",
            "agent-browser --cdp 9222 snapshot",
            "agent-browser state load s.json",
            "agent-browser --json state save s.json",
            "agent-browser connect 9222",
            "agent-browser open https://a.in --args \"--user-data-dir=/x\"",
            "agent-browser open https://a.in --restore",
            "agent-browser open https://a.in --restore-check-fn x",
            "agent-browser open https://a.in --allow-file-access",
            "agent-browser open https://a.in --download-path /tmp",
            "agent-browser open https://a.in --proxy http://p:1",
            "agent-browser --namespace n open https://a.in",
            "agent-browser --provider browserbase open https://a.in",
            "cd /tmp && agent-browser open https://a.in --profile p",
            "agent-browser snapshot && agent-browser eval 1 --profile p",
        ],
    );
}

/// Every subcommand that reads or writes a local file or runs page JavaScript asks wherever it
/// sits: bare, behind `--json`, behind `-q` or another global option, with a full path.
#[test]
fn file_and_javascript_subcommands_ask_behind_any_option() {
    let data = "\"/Users/a/Library/Application Support/ai.clauseo.desktop/agents/auth.json\"";
    let subs = [
        format!("upload @f {data}"),
        "eval \"document.cookie\"".to_owned(),
        format!("pdf {data}"),
        format!("download @e1 {data}"),
        format!("state load {data}"),
        format!("state save {data}"),
        "wait --fn \"window.ok\"".to_owned(),
        "wait -f x".to_owned(),
        "get cdp-url".to_owned(),
    ];
    let lead = [
        "",
        "--json ",
        "-q ",
        "--max-output 5 ",
        "-q --json ",
        "--json -q ",
        "--timeout=3 -q ",
    ];
    for sub in &subs {
        for lead in lead {
            let command = format!("agent-browser {lead}{sub}");
            // Bare `state` stays denied; behind another option it asks.
            let want = if sub.starts_with("state ") && matches!(lead, "" | "--json ") {
                Verdict::Deny
            } else {
                Verdict::Ask
            };
            assert_eq!(verdict(&command), want, "{command}");
            let chained = format!("agent-browser open https://a.in && {command}");
            assert_eq!(verdict(&chained), want, "{chained}");
        }
    }
    // A word that only holds one of them, or the allowed `wait --download`, does not ask.
    assert_all(
        Verdict::Allow,
        &[
            "agent-browser fill @e2 \"medieval\"",
            "agent-browser wait --download",
            "agent-browser fill @e2 \"state bank\"",
        ],
    );
}

/// Forms in no rule, which the mode decides: another global option first, a full path, `npx`,
/// a subcommand outside the list, or a chain with another command.
#[test]
fn other_forms_are_left_to_the_mode() {
    assert_all(
        Verdict::Free,
        &[
            "agent-browser -q open https://a.in",
            "/opt/clauseo/agent-browser open https://a.in",
            "npx agent-browser open https://a.in",
            "agent-browser cookies",
            "agent-browser keyboard type x",
            "agent-browser snapshot | head -50",
            "agent-browser open https://a.in && curl https://b.in",
        ],
    );
}
