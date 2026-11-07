//! Round 6's D1: the Codex fence's deny globs run through the real ripgrep command bubblewrap
//! uses to expand them (codex-rs `linux-sandbox/src/bwrap.rs`, `split_pattern_for_ripgrep` at
//! :746-773, `expand_unreadable_globs_with_ripgrep` at :700-744, `ripgrep_files` at :810-850),
//! from a working folder that is not `agent-state`, as the sandbox helper starts in the command's
//! folder. The split is mirrored here (codex-rs is not a dependency); ripgrep is the system's.
//! Skipped when `rg` is not installed (CI has it).

#![expect(
    clippy::disallowed_methods,
    reason = "runs the system's ripgrep as Codex's sandbox does, and times it by hand"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::STATE_GLOBS;
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// codex-rs `split_pattern_for_ripgrep` (bwrap.rs:746-773): the static part before the first
/// glob character, up to its last `/`, is the search root; the rest is ripgrep's glob.
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
/// `rg --files --hidden --no-ignore --null --glob …` per root, run in `cwd`. `None` without rg.
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
            .args(["--files", "--hidden", "--no-ignore", "--null"]);
        for glob in &globs {
            command.arg("--glob").arg(glob);
        }
        command.arg("--").arg(&root);
        let output = match command.output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
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
/// run from a working folder elsewhere; the own state folder's `tmp` and the matter's notes are
/// matched by none.
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
        later.join("claude-acp/.credentials.json"),
        later.join("claude-acp/.claude.json"),
    ];
    let own = data.join("agent-state").join("m1");
    let spared = [
        own.join("codex-acp/tmp/arg0/codex-arg0abc/apply_patch"),
        own.join("notes/n.md"),
        later.join("notes/n.md"),
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

/// The cost of the globs at each sandboxed command, timed by hand (ignored): 20 matters, each
/// with a 5,000-file `tmp`, a 2,000-file `notes` and a few transcripts; all patterns share one
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
        for (folder, count) in [("codex-acp/tmp", 5_000), ("notes", 2_000)] {
            let folder = root.join(folder);
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
