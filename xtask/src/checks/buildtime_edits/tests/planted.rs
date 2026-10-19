//! The planted cases of the buildtime-edits check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A clean `ui/app/src`, with a nested file and no attribute, passes.
    clean_src_passes: Passes => super::clean_src_passes,
    /// An attribute left in `App.tsx` with the lock free fails at its line, worded as a leftover.
    left_attribute: FailsWith(&[
        "ui/app/src/App.tsx:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]) => super::left_attribute,
    /// Two lines of one file that hold the attribute fail one at a time.
    two_lines_in_one_file: FailsWith(&[
        "ui/app/src/App.tsx:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
        "ui/app/src/App.tsx:3: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]) => super::two_lines_in_one_file,
    /// The attribute in a file nested under `ui/app/src/views/` fails at its line.
    nested_file: FailsWith(&[
        "ui/app/src/views/V.tsx:2: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]) => super::nested_file,
    /// The attribute in a file that is not UTF-8 still fails: the bytes are read lossily.
    non_utf8_file: FailsWith(&[
        "ui/app/src/logo.bin:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]) => super::non_utf8_file,
    /// The attribute while the buildtime lock is held is a live run, not a leftover.
    live_run_holds_the_edit: FailsWith(&[
        "ui/app/src/App.tsx:1: a `cargo xtask buildtime` run is in progress and holds this edit\n    \
         fix: rerun the check when that run ends",
    ]) => super::live_run_holds_the_edit,
    /// A `ui/app/src` that is missing inspects nothing, and the zero line names the fix.
    missing_src: FailsWith(&[
        "0 UI source files inspected under ui/app/src\n    \
         fix: buildtime-edits looks for UI source files under ui/app/src; restore them, or update \
         xtask/src/checks/buildtime_edits.rs if they moved",
    ]) => super::missing_src,
    /// An empty `ui/app/src` inspects nothing the same way.
    empty_src: FailsWith(&[
        "0 UI source files inspected under ui/app/src\n    \
         fix: buildtime-edits looks for UI source files under ui/app/src; restore them, or update \
         xtask/src/checks/buildtime_edits.rs if they moved",
    ]) => super::empty_src,
}
