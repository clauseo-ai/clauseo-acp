//! Round 6's D1: the Codex fence's deny globs run through the real ripgrep command bubblewrap
//! uses to expand them, from a working folder that is not `agent-state`, as the sandbox helper
//! starts in the command's folder. ripgrep is the system's; the split and the command line are
//! mirrored, since codex-rs is not a dependency. The mirror is pinned to the Codex the host
//! runs, rust-v0.162.1, `codex-rs/linux-sandbox/src/bwrap.rs`:
//! - `MAX_UNREADABLE_GLOB_MATCHES` at :62;
//! - `expand_unreadable_globs_with_ripgrep` at :891 (the grouping by root, the cap);
//! - `split_pattern_for_ripgrep` at :947-974 (`escape_unclosed_glob_classes` at :976 is not
//!   mirrored: no pattern here holds `[`);
//! - `ripgrep_files` at :1011-1033 (`--no-config --files --hidden --no-ignore --null`,
//!   `--max-depth` from the profile's `glob_scan_max_depth`, read at :504-505).
//!
//! A Codex bump re-reads those lines.
//! Without `rg` the tests are skipped by hand, but fail when `CI` is set, so they never skip
//! quietly there.

// Codex runs ripgrep on Linux only (bubblewrap); Seatbelt matches the globs as regexes, and
// Windows has no fence.
#![cfg(target_os = "linux")]
#![expect(
    clippy::disallowed_methods,
    reason = "runs the system's ripgrep as Codex's sandbox does, and times it by hand"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{GLOB_SCAN_MAX_DEPTH, STATE_GLOBS};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// codex-rs `split_pattern_for_ripgrep` (bwrap.rs:947-974 at rust-v0.162.1): the static part
/// before the first glob character, up to its last `/`, is the search root; the rest is
/// ripgrep's glob.
fn split(pattern: &str) -> Option<(PathBuf, String)> {
    let first = pattern.find(['*', '?', '[', ']'])?;
    let prefix = pattern.get(..first)?;
    if prefix.is_empty() || prefix == "/" {
        return None;
    }
    let end = if prefix.ends_with('/') {
        prefix.len() - 1
    } else {
        prefix.rfind('/').unwrap_or(0)
    };
    let root = if end == 0 {
        PathBuf::from("/")
    } else {
        PathBuf::from(pattern.get(..end)?)
    };
    let glob = pattern.get(end + 1..)?.to_owned();
    (!glob.is_empty()).then_some((root, glob))
}

/// The fence's globs as Codex expands them under `data`: grouped by search root, one
/// `rg --no-config --files --hidden --no-ignore --null --max-depth <GLOB_SCAN_MAX_DEPTH>
/// --glob …` per root, run in `cwd`. `None` without rg.
fn expand(data: &Path, cwd: &Path) -> Option<Vec<PathBuf>> {
    let mut by_root: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for glob in STATE_GLOBS {
        let pattern = format!("{}/{glob}", data.display());
        let (root, glob) = split(&pattern).expect("a pattern with a static prefix");
        by_root.entry(root).or_default().push(glob);
    }
    let mut found = Vec::new();
    for (root, globs) in by_root {
        let mut command = Command::new("rg");
        command
            .current_dir(cwd)
            .args([
                "--no-config",
                "--files",
                "--hidden",
                "--no-ignore",
                "--null",
            ])
            .arg("--max-depth")
            .arg(GLOB_SCAN_MAX_DEPTH.to_string());
        for glob in &globs {
            command.arg("--glob").arg(glob);
        }
        command.arg("--").arg(&root);
        let output = match command.output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                assert!(
                    std::env::var_os("CI").is_none(),
                    "ripgrep (rg) is not installed on this CI runner: the deny-glob tests need it"
                );
                return None;
            }
            Err(error) => panic!("rg: {error}"),
        };
        assert!(
            output.status.success() || output.status.code() == Some(1),
            "rg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        found.extend(
            output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned())),
        );
    }
    Some(found)
}

/// Every planted login file of a matter made after the launch is matched by the real command,
/// run from a working folder elsewhere; the own state folder's `tmp`, the matters' notes and
/// Claude's state (outside `agent-state` since R17 PR-D1, denied whole by `rules`) are matched
/// by none.
#[test]
fn the_deny_globs_match_a_later_matters_logins_from_any_working_folder() {
    let dir = TempDir::new("codex-fence-rg");
    let data = dir.path().join("data");
    let later = data.join("agent-state").join("m-later");
    let secrets = [
        later.join("codex-acp/auth.json"),
        later.join("codex-acp/.env"),
        later.join("codex-acp/config.toml"),
        later.join("codex-acp/history.jsonl"),
        later.join("codex-acp/shell_snapshots/s.sh"),
    ];
    let own = data.join("agent-state").join("m1");
    let spared = [
        own.join("codex-acp/tmp/arg0/codex-arg0abc/apply_patch"),
        data.join("notes/m1/n.md"),
        data.join("notes/m-later/n.md"),
        data.join("claude-state/m-later/claude-acp/.claude.json"),
    ];
    for file in secrets.iter().chain(&spared) {
        write::create_dir_all(file.parent().unwrap()).unwrap();
        write::write_file(file, b"x").unwrap();
    }
    let elsewhere = dir.path().join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    let Some(found) = expand(&data, &elsewhere) else {
        println!("rg is not installed: skipped");
        return;
    };
    for secret in &secrets {
        assert!(
            found.contains(secret),
            "{} not matched: {found:?}",
            secret.display()
        );
    }
    for file in &spared {
        assert!(!found.contains(file), "{} matched", file.display());
    }
}

/// The matches a matter's state adds after the clean, with a live session: Codex's login link
/// and `config.toml`, and one live shell snapshot. `.env` is cleaned and `history.jsonl` is
/// the TUI's only; Claude's state is outside `agent-state` (R17 PR-D1).
const MATCHES_PER_MATTER: usize = 3;

/// The names under `agent-state/<matter>` the globs can match within [`GLOB_SCAN_MAX_DEPTH`]
/// (4), whoever writes them: the 4 login names at depth 3 (`codex-acp/{auth.json,.env,
/// config.toml,history.jsonl}`), and the same 4 one folder down, under the matter folder's one
/// entry (`codex-acp`), which only the host makes (`**/` spans one folder at depth 4). No
/// shell and no Claude writes under `agent-state` since R17 PR-D1 (the notes and Claude's state
/// left it). Plus that matter's live shell snapshots (`codex-acp/shell_snapshots/*`, depth 4),
/// which Codex's own process writes, one per live session here.
const PLANTABLE_PER_MATTER: usize = 4 + 4 + 1;

/// Codex fails every sandboxed command past 8192 glob matches (`MAX_UNREADABLE_GLOB_MATCHES`,
/// bwrap.rs:62).
const CODEX_GLOB_CAP: usize = 8192;

/// The 8192-cap headroom and the depth cap, through the real rg with Codex's split mirrored
/// (rust-v0.162.1, pinned above) and its cap as the constant 8192
/// (`MAX_UNREADABLE_GLOB_MATCHES`), not through Codex's own code. One matter is heavy
/// with files that are not logins (hundreds of transcripts, logs, notes, tool results; the notes
/// and Claude's outside `agent-state`), has every name the globs can match within the depth cap,
/// and has plants one folder deeper: 300 `codex-acp/x<n>/codex-acp/auth.json` (stage C's
/// cross-matter shutdown) and a snapshot in a subfolder. Only the [`PLANTABLE_PER_MATTER`]
/// names match, so no matter can push the others past the cap; a matter with only its own files
/// matches [`MATCHES_PER_MATTER`], so 2,000 matters stay under it.
#[test]
fn the_deny_globs_stay_under_codexs_match_cap() {
    let dir = TempDir::new("codex-fence-rg-cap");
    let data = dir.path().join("data");
    let matter = data.join("agent-state").join("m-heavy");
    let quiet = data.join("agent-state").join("m-quiet");
    let logins = [
        "codex-acp/auth.json",
        "codex-acp/.env",
        "codex-acp/config.toml",
        "codex-acp/history.jsonl",
    ];
    let mut matching: Vec<PathBuf> = logins.iter().map(|name| matter.join(name)).collect();
    // The matter folder's one entry, `codex-acp`, one folder down.
    matching.extend(
        logins
            .iter()
            .map(|name| matter.join("codex-acp").join(name)),
    );
    matching.push(matter.join("codex-acp/shell_snapshots/live.sh"));
    let own: Vec<PathBuf> = [
        "codex-acp/auth.json",
        "codex-acp/config.toml",
        "codex-acp/shell_snapshots/live.sh",
    ]
    .iter()
    .map(|name| quiet.join(name))
    .collect();
    let mut others = vec![
        matter.join("codex-acp/shell_snapshots/deeper/s.sh"),
        matter.join("codex-acp/state_5.sqlite"),
        matter.join("codex-acp/logs_2.sqlite"),
    ];
    for n in 0..300 {
        others.push(matter.join(format!("codex-acp/x{n}/codex-acp/auth.json")));
        others.push(matter.join(format!("codex-acp/sessions/2026/11/07/rollout-{n}.jsonl")));
        others.push(matter.join(format!("codex-acp/log/codex-{n}.log")));
        others.push(data.join(format!(
            "claude-state/m-heavy/claude-acp/projects/p/s-{n}.jsonl"
        )));
        others.push(data.join(format!("notes/m-heavy/x{n}/codex-acp/auth.json")));
        others.push(quiet.join(format!("codex-acp/tmp/arg0/codex-arg0{n}/apply_patch")));
    }
    for file in matching.iter().chain(&own).chain(&others) {
        write::create_dir_all(file.parent().unwrap()).unwrap();
        write::write_file(file, b"x").unwrap();
    }
    let elsewhere = dir.path().join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    let Some(mut found) = expand(&data, &elsewhere) else {
        println!("rg is not installed: skipped");
        return;
    };
    found.sort();
    let mut expected: Vec<PathBuf> = matching.iter().chain(&own).cloned().collect();
    expected.sort();
    assert_eq!(found, expected);
    assert_eq!(matching.len(), PLANTABLE_PER_MATTER);
    assert_eq!(own.len(), MATCHES_PER_MATTER);
    const { assert!(MATCHES_PER_MATTER * 2_000 < CODEX_GLOB_CAP) };
    const { assert!(PLANTABLE_PER_MATTER * 300 < CODEX_GLOB_CAP) };
}

/// The cost of the globs at each sandboxed command, timed by hand (ignored): 20 matters, each
/// with a 5,000-file `tmp`, a 2,000-file `notes` (outside `agent-state`, so not walked) and a
/// few transcripts; all patterns share one
/// search root, so one ripgrep walk. Prints the median of 9 runs.
///
/// ```text
/// cargo nextest run -p clauseo-host --run-ignored only \
///     -E 'test(timing_the_deny_globs_ripgrep_walk)' --no-capture
/// ```
#[test]
#[ignore = "a timing run, by hand"]
fn timing_the_deny_globs_ripgrep_walk() {
    let dir = TempDir::new("codex-fence-rg-timing");
    let data = dir.path().join("data");
    for matter in 0..20 {
        let root = data.join("agent-state").join(format!("m{matter}"));
        let notes = data.join("notes").join(format!("m{matter}"));
        for (folder, count) in [(root.join("codex-acp/tmp"), 5_000), (notes, 2_000)] {
            write::create_dir_all(&folder).unwrap();
            for n in 0..count {
                write::write_file(&folder.join(format!("f{n}")), b"x").unwrap();
            }
        }
        for name in ["codex-acp/auth.json", "codex-acp/config.toml"] {
            write::write_file(&root.join(name), b"x").unwrap();
        }
        let sessions = root.join("codex-acp/sessions/2026/11/07");
        write::create_dir_all(&sessions).unwrap();
        for n in 0..5 {
            write::write_file(&sessions.join(format!("r{n}.jsonl")), b"x").unwrap();
        }
    }
    let elsewhere = dir.path().join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    let mut times = Vec::new();
    let mut matches = 0;
    for _ in 0..9 {
        let start = std::time::Instant::now();
        let Some(found) = expand(&data, &elsewhere) else {
            println!("rg is not installed: skipped");
            return;
        };
        times.push(start.elapsed());
        matches = found.len();
    }
    times.sort();
    println!(
        "deny globs: one rg walk over 20 matters (140,000 files): median {:?}, max {:?}, {matches} matches",
        times[times.len() / 2],
        times[times.len() - 1]
    );
}
