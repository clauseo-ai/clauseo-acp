//! Each shell call starts in the matter's folder: Clauseo launches Claude with
//! `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1`, which puts its shell back there after every
//! call, so a `cd` in one call never moves where the next call's relative write lands.

use std::path::Path;

use crate::lawyer_file::{Answer, answered_turn, flag};
use crate::support::START_MS;
use crate::write;

/// Dates `file` a minute before the turn's end, as the agent's own new file is.
fn before_end(file: &Path) {
    let at = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis(u64::try_from(START_MS - 60_000).unwrap());
    write::set_mtime(file, at);
}

/// `cd "<matter>/Sub"` in one call, `pandoc a.md -o x.docx` in the next: the next call starts
/// in the matter's folder again, so the root `x.docx` is the agent's and the `Sub/x.docx` the
/// lawyer edited meanwhile stays flagged.
#[test]
fn a_cd_in_one_call_does_not_move_the_next_calls_write() {
    let files = [("a.md", "# Reply\n"), ("Sub/x.docx", "the lawyer's x\n")];
    let (_dir, host, _, _, _, flags) = answered_turn(
        "shell_folder_reset",
        "shell-folder-reset",
        &files,
        |folder| {
            write::write(&folder.join("x.docx"), b"the agent's reply\n");
            before_end(&folder.join("x.docx"));
            write::write(&folder.join("Sub/x.docx"), b"the lawyer's x, edited\n");
        },
        Answer::Allow,
    );
    assert_eq!(flags, [flag("Sub/x.docx", true), flag("x.docx", false)]);
    host.shutdown();
}
