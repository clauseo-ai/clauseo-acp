//! What a delete or a move ([`super::deletes::Taken`]) adds to a reading: the words at its
//! places, a PowerShell array as each of its elements, joined to the folder the command runs
//! in, and, for [`Kind::Changes`], where a move puts its sources.

use crate::pre_write_guard::ARRAY_COMMA;

use super::deletes::{Act, Arg, Taken, To};
use super::{How, Kind, Out, Word};

/// The paths the word at `arg` holds from its byte `arg.from` on: the word itself when it is
/// plain text, each element of a PowerShell array of plain text; `None` for any other word or an
/// empty path.
fn parts(args: &[Word], arg: Arg) -> Option<Vec<&str>> {
    let word = args.get(arg.at)?;
    let text = word.text.get(arg.from..)?;
    let parts: Vec<&str> = if word.clear {
        vec![text]
    } else if word.list {
        text.split(ARRAY_COMMA).collect()
    } else {
        return None;
    };
    parts.iter().all(|part| !part.is_empty()).then_some(parts)
}

/// Adds what `taken` names among `args`, run in `cwd`, as `how.kind` reads it: what a delete
/// names, what a move takes away, or, for [`Kind::Changes`], both and where a move puts them.
/// Marks `out` unclear when a delete or a move it reads names a target that is no path here.
pub(super) fn add_taken(
    out: &mut Out<'_>,
    cwd: Option<&str>,
    args: &[Word],
    taken: &Taken,
    how: How,
) {
    let wanted = match how.kind {
        Kind::Writes => return,
        Kind::Deletes => Act::Delete,
        Kind::MovedFrom => Act::Move,
        Kind::Changes => match taken.act {
            Some(act) => act,
            None => return,
        },
    };
    if taken.act != Some(wanted) {
        return;
    }
    // A cmdlet that names no path takes its paths from elsewhere (a pipeline's input).
    if taken.unclear || taken.input {
        out.unclear = true;
    }
    for arg in taken.deleted.iter().chain(&taken.sources) {
        let Some(parts) = parts(args, *arg) else {
            out.unclear = true;
            continue;
        };
        for part in parts {
            // A folder named with a separator at its end (`rm -r Exhibits/`) is that folder.
            let trimmed = part.trim_end_matches(['/', '\\']);
            if !out.path(cwd, if trimmed.is_empty() { "/." } else { trimmed }) {
                out.unclear = true;
            }
        }
    }
    if how.kind == Kind::Changes {
        add_destinations(out, cwd, args, taken);
    }
}

/// Adds where a move puts its sources: a destination path, matched only whole, and each
/// source's name in it, which may be a folder; a new path (`mv -T`); each source's name in a
/// destination folder; a rename's new name beside each source.
fn add_destinations(out: &mut Out<'_>, cwd: Option<&str>, args: &[Word], taken: &Taken) {
    let Some(to) = taken.to else {
        return;
    };
    let sources: Vec<&str> = taken
        .sources
        .iter()
        .filter_map(|arg| parts(args, *arg))
        .flatten()
        .collect();
    let one = |arg: Arg| match parts(args, arg).as_deref() {
        Some([path]) => Some(*path),
        _ => None,
    };
    match to {
        To::Path(arg) => {
            if let Some(path) = one(arg) {
                out.whole(cwd, path);
                into(out, cwd, path, &sources);
            }
        }
        To::New(arg) => {
            if let Some(path) = one(arg) {
                out.path(cwd, path);
            }
        }
        To::Folder(arg) => {
            if let Some(folder) = one(arg) {
                into(out, cwd, folder, &sources);
            }
        }
        To::Beside(arg) => {
            let Some(new) = one(arg) else {
                return;
            };
            if new.contains(['/', '\\']) {
                out.path(cwd, new);
                return;
            }
            for source in &sources {
                let (folder, _) = split_last(source.trim_end_matches(['/', '\\']));
                out.path(cwd, &format!("{folder}{new}"));
            }
        }
    }
}

/// `path` split before its last part: `Drafts\a.docx` is (`Drafts\`, `a.docx`), a
/// drive-relative `C:a.docx` is (`C:`, `a.docx`), and `a.docx` is an empty first part and
/// `a.docx`.
fn split_last(path: &str) -> (&str, &str) {
    let drive = path
        .get(..2)
        .filter(|head| head.ends_with(':') && head.starts_with(|c: char| c.is_ascii_alphabetic()));
    let cut = path
        .rfind(['/', '\\'])
        .map(|at| at + 1)
        .or(drive.map(str::len))
        .unwrap_or(0);
    path.split_at_checked(cut).unwrap_or(("", path))
}

/// Adds each of `sources` under its own name in `folder`.
fn into(out: &mut Out<'_>, cwd: Option<&str>, folder: &str, sources: &[&str]) {
    let folder = folder.trim_end_matches(['/', '\\']);
    for source in sources {
        let (_, name) = split_last(source.trim_end_matches(['/', '\\']));
        out.path(cwd, &format!("{folder}/{name}"));
    }
}
