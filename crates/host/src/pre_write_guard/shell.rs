//! A shell command's targets: its words, segment by segment, each candidate name taken from the
//! folder the command has `cd`'d to by then (lexically, as the shell's default `cd` does) and from
//! the hook's `cwd`.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::words::{Escape, Segments, candidates, shell_segments};
use super::{
    Ignored, MAX_WORDS, Targets, existing_resolved, lexical, patch_targets, text_has_codex_part,
};

/// Where a command's working folder is after its segments so far.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Folder {
    /// A folder the guard followed the command to.
    Known(PathBuf),
    /// A `cd` the guard cannot follow (`cd "$(…)"`, `cd -`, `popd` with nothing pushed).
    Unknown,
}

/// A shell command's targets: the existing files among its first [`MAX_WORDS`] words, the files
/// of a patch in its text, and whether one of those words has a `.codex` part. Claude Code sends
/// the command as a string; an argument list (one shape Codex has used) is taken word by word,
/// each split again so `["bash", "-lc", "…"]` reaches the script's words. Each word's
/// candidates ([`candidates`]) are taken from `cwd` and from the folder the segments before it
/// moved to ([`moved`]); after a `cd` the guard cannot follow, a relative candidate is also
/// taken from `cwd` with its leading `..` and `.` parts off. Each of those paths counts as
/// spelled and normalised ([`resolutions`]). A command of another JSON type is ignored.
pub(super) fn shell_targets(
    command: Option<&Value>,
    cwd: &Path,
    escape: Escape,
) -> Result<Targets, Ignored> {
    let mut segments = Segments::new(MAX_WORDS);
    let mut text = String::new();
    match command {
        Some(Value::String(command)) => {
            shell_segments(command, escape, &mut segments);
            text.push_str(command);
        }
        Some(Value::Array(parts)) => {
            for part in parts.iter().filter_map(Value::as_str) {
                text.push_str(part);
                text.push('\n');
                segments.split();
                segments.push(part.to_owned());
                shell_segments(part, escape, &mut segments);
            }
        }
        _ => return Err(Ignored::Shape("command string or list")),
    }
    let mut files: Vec<PathBuf> = Vec::new();
    let mut names_codex = false;
    let mut folder = Folder::Known(cwd.to_path_buf());
    let mut pushed: Vec<Folder> = Vec::new();
    for segment in segments.iter() {
        for word in segment {
            for candidate in candidates(word) {
                names_codex |= text_has_codex_part(candidate);
                let candidate = native(candidate, escape);
                for path in starts(&folder, cwd, Path::new(candidate.as_ref())) {
                    for path in resolutions(&path) {
                        if !files.contains(&path) && path.metadata().is_ok_and(|m| m.is_file()) {
                            files.push(path);
                        }
                    }
                }
            }
        }
        folder = moved(folder, segment, escape, &mut pushed);
    }
    Ok(Targets::Shell {
        files,
        patch: patch_targets(&text),
        names_codex,
    })
}

/// `path` as spelled and normalised: `..` and `.` parts resolved by their text first, then its
/// deepest existing folder canonicalised and the rest added. The first is what the OS opens when
/// every folder exists (a symlinked folder's `..` included); the second catches a `..` after a
/// folder the command makes first (`mkdir -p D && echo x > D/../Contract.docx`), or never does.
/// A path with no `.` or `..` part names the same file both ways, so it comes once.
pub(super) fn resolutions(path: &Path) -> Vec<PathBuf> {
    let normal = lexical(path);
    if normal == path {
        vec![normal]
    } else {
        vec![path.to_path_buf(), existing_resolved(&normal)]
    }
}

/// `candidate` with its separators as the OS reads them: PowerShell takes `\` as a separator on
/// every OS, so off Windows it becomes `/`.
fn native(candidate: &str, escape: Escape) -> Cow<'_, str> {
    if escape == Escape::Backtick && !cfg!(windows) && candidate.contains('\\') {
        Cow::Owned(candidate.replace('\\', "/"))
    } else {
        Cow::Borrowed(candidate)
    }
}

/// The paths `candidate` may name: from `folder` when known, from `cwd`, and when `folder` is
/// unknown also from `cwd` with its leading `..` and `.` parts off (`cd "$(…)" && echo x >
/// ../Contract.docx` most likely went one folder down first).
fn starts(folder: &Folder, cwd: &Path, candidate: &Path) -> Vec<PathBuf> {
    let mut paths = vec![cwd.join(candidate)];
    match folder {
        Folder::Known(folder) if folder != cwd => paths.push(folder.join(candidate)),
        Folder::Known(_) => {}
        Folder::Unknown => {
            let rest: PathBuf = candidate
                .components()
                .skip_while(|part| matches!(part, Component::ParentDir | Component::CurDir))
                .collect();
            if !rest.as_os_str().is_empty() && rest != candidate {
                paths.push(cwd.join(rest));
            }
        }
    }
    paths
}

/// The working folder after `segment` runs from `folder`. A segment that changes folder starts
/// with `cd`, `chdir` or `pushd` (POSIX shells), or `Set-Location`, `sl`, `cd`, `chdir`,
/// `Push-Location` or `pushd` (PowerShell), ignoring case and a leading `(` or `{`; its folder is
/// its first word not starting with `-`, or the part after `:` of a `-Path:x` word. No folder,
/// or one starting with `~`, leaves it (home is not the matter); a folder with `$`, `` ` ``, a
/// glob or `-` alone is unknown; else it is joined to `folder` and normalised by its text. `popd`
/// and `Pop-Location` go back to the folder `pushd` left, unknown when none.
fn moved(folder: Folder, segment: &[String], escape: Escape, pushed: &mut Vec<Folder>) -> Folder {
    let Some((command, args)) = segment.split_first() else {
        return folder;
    };
    let command = command.trim_start_matches(['(', '{']).to_ascii_lowercase();
    let push = match command.as_str() {
        "pushd" | "push-location" => true,
        "cd" | "chdir" | "set-location" | "sl" => false,
        "popd" | "pop-location" => return pushed.pop().unwrap_or(Folder::Unknown),
        _ => return folder,
    };
    let target = args.iter().find_map(|arg| match arg.strip_prefix('-') {
        Some(option) if !option.is_empty() => option
            .split_once(':')
            .map(|(_, value)| value.trim_matches(['\'', '"'])),
        Some(_) | None => Some(arg.as_str()),
    });
    let target = target.map(|target| target.trim_end_matches([')', '}', '&']));
    let next = match target {
        None => folder.clone(),
        Some(target) if target.starts_with('~') => folder.clone(),
        Some(target) if target == "-" || target.contains(['$', '`', '*', '?', '[']) => {
            Folder::Unknown
        }
        Some(target) => {
            let target = native(target, escape);
            let target = Path::new(target.as_ref());
            match &folder {
                _ if target.is_absolute() => Folder::Known(lexical(target)),
                Folder::Known(folder) => Folder::Known(lexical(&folder.join(target))),
                Folder::Unknown => Folder::Unknown,
            }
        }
    };
    if push {
        pushed.push(folder);
    }
    next
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Escape, Folder, moved};

    /// The folder after each of `segments` (words split at spaces), from `/m`.
    fn walk(segments: &[&str], escape: Escape) -> Folder {
        let mut folder = Folder::Known(PathBuf::from("/m"));
        let mut pushed = Vec::new();
        for segment in segments {
            let words: Vec<String> = segment.split(' ').map(str::to_owned).collect();
            folder = moved(folder, &words, escape, &mut pushed);
        }
        folder
    }

    fn known(path: &str) -> Folder {
        Folder::Known(Path::new(path).to_path_buf())
    }

    #[cfg(unix)]
    #[test]
    fn cd_moves_the_folder_by_its_text() {
        let posix = Escape::Backslash;
        assert_eq!(walk(&["cd D"], posix), known("/m/D"));
        assert_eq!(walk(&["cd D", "cd .."], posix), known("/m"));
        assert_eq!(walk(&["cd ./D/../E/"], posix), known("/m/E"));
        assert_eq!(walk(&["cd -P D"], posix), known("/m/D"));
        assert_eq!(walk(&["(cd D)"], posix), known("/m/D"));
        assert_eq!(walk(&["cd /x/y"], posix), known("/x/y"));
        assert_eq!(walk(&["cd"], posix), known("/m"));
        assert_eq!(walk(&["cd ~/x"], posix), known("/m"));
        assert_eq!(walk(&["echo cd D"], posix), known("/m"));
        assert_eq!(walk(&["cd $(pwd)/D"], posix), Folder::Unknown);
        assert_eq!(walk(&["cd -"], posix), Folder::Unknown);
        assert_eq!(walk(&["cd $(pwd)", "cd D"], posix), Folder::Unknown);
        assert_eq!(walk(&["cd $(pwd)", "cd /m"], posix), known("/m"));
        assert_eq!(walk(&["pushd D", "cd E", "popd"], posix), known("/m"));
        assert_eq!(walk(&["popd"], posix), Folder::Unknown);
    }

    #[cfg(unix)]
    #[test]
    fn powershell_set_location_moves_the_folder() {
        let ps = Escape::Backtick;
        assert_eq!(walk(&["Set-Location D"], ps), known("/m/D"));
        assert_eq!(walk(&["set-location -Path D\\E"], ps), known("/m/D/E"));
        assert_eq!(walk(&["Set-Location -LiteralPath:'D'"], ps), known("/m/D"));
        assert_eq!(walk(&["sl ..\\x"], ps), known("/x"));
        assert_eq!(walk(&["Push-Location D", "Pop-Location"], ps), known("/m"));
    }
}
