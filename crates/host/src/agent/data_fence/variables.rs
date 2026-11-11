//! PowerShell's environment variables in a path word, for the data folder fence. Under Windows'
//! rules `\` is no escape, so the `\$env:Path` that Git Bash hands PowerShell as `$env:Path`
//! is a path word to the fence, and its name holds a `:`, which the name rule refuses as an
//! alternate data stream (`network_paths::refused_windows_name`): the 2026-11-11 dry run's
//! `powershell -NoProfile -Command "\$env:Path -split ';'"` was refused unseen. Read as a
//! path, the text opens a stream of the file before the `:` (`$env`, `${env`), so a name shaped
//! like the variable is judged as that file instead: resolved, links followed and compared
//! with the data folder like any other. A stream with a type (a second `:`), a bare `$env:`
//! and every other `:` name stay refused untouched.

use std::path::{Path, PathBuf};

/// `path` with each name that is PowerShell's environment variable ([`variable_file`]) cut to
/// the file Windows would open for it, with Windows' rules (`windows`); as it is otherwise, or
/// when it is not UTF-8.
pub(super) fn file_of_variables(path: &Path, windows: bool) -> PathBuf {
    let Some(text) = path.to_str().filter(|_| windows) else {
        return path.to_owned();
    };
    let mut out = String::with_capacity(text.len());
    for piece in text.split_inclusive(['/', '\\']) {
        let name = piece.trim_end_matches(['/', '\\']);
        out.push_str(variable_file(name).unwrap_or(name));
        out.push_str(piece.get(name.len()..).unwrap_or_default());
    }
    PathBuf::from(out)
}

/// The file part of `name` (before its `:`) when it starts with PowerShell's `$env:NAME` or
/// `${env:NAME}` (`env` in any case, `NAME` letters, digits and `_`), and holds no other `:`.
fn variable_file(name: &str) -> Option<&str> {
    let (file, stream) = name.split_once(':')?;
    let braced = if file.eq_ignore_ascii_case("$env") {
        false
    } else if file.eq_ignore_ascii_case("${env") {
        true
    } else {
        return None;
    };
    let rest = stream.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
    let named = rest.len() < stream.len();
    let closed = !braced || rest.starts_with('}');
    (named && closed && !rest.contains(':')).then_some(file)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::file_of_variables;

    #[test]
    fn only_a_variable_shaped_name_is_cut_and_only_with_windows_rules() {
        let cut = |text: &str| file_of_variables(Path::new(text), true);
        assert_eq!(cut(r"\$env:Path -split ';'"), PathBuf::from(r"\$env"));
        assert_eq!(cut(r"\${ENV:Path} -split ';'"), PathBuf::from(r"\${ENV"));
        assert_eq!(
            cut(r"C:\x\$Env:PATH.Split(\y"),
            PathBuf::from(r"C:\x\$Env\y")
        );
        for kept in [
            r"\$env:",
            r"\${env:Path",
            r"\$env:Path:$DATA",
            r"\a.txt:Path",
            r"\env:Path",
            r"C:\x",
            r"D:",
        ] {
            assert_eq!(cut(kept), PathBuf::from(kept), "{kept}");
        }
        let unix = file_of_variables(Path::new("/m/$env:Path"), false);
        assert_eq!(unix, PathBuf::from("/m/$env:Path"));
    }
}
