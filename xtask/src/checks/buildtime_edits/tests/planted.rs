//! The planted cases of the buildtime-edits check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A clean `ui/app/src`, with a nested file and no attribute, passes.
    clean_src_passes: Passes, inspects 2 => super::clean_src_passes,
    /// An attribute left in `App.tsx` with the lock free fails at its line, worded as a leftover.
    left_attribute: FailsWith(&[
        "ui/app/src/App.tsx:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 1 => super::left_attribute,
    /// Two lines of one file that hold the attribute fail one at a time.
    two_lines_in_one_file: FailsWith(&[
        "ui/app/src/App.tsx:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
        "ui/app/src/App.tsx:3: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 1 => super::two_lines_in_one_file,
    /// The attribute in a file nested under `ui/app/src/views/` fails at its line.
    nested_file: FailsWith(&[
        "ui/app/src/views/V.tsx:2: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 2 => super::nested_file,
    /// The attribute in a file that is not UTF-8 still fails: the bytes are read lossily.
    non_utf8_file: FailsWith(&[
        "ui/app/src/logo.bin:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 1 => super::non_utf8_file,
    /// The attribute while the buildtime lock is held is a live run, not a leftover.
    live_run_holds_the_edit: FailsWith(&[
        "ui/app/src/App.tsx:1: a `cargo xtask buildtime` run is in progress and holds this edit\n    \
         fix: rerun the check when that run ends",
    ]), inspects 1 => super::live_run_holds_the_edit,
    /// A `ui/app/src` that is missing inspects nothing, and the zero line names the fix.
    missing_src: FailsWith(&[
        "0 UI source files inspected under ui/app/src\n    \
         fix: buildtime-edits looks for UI source files under ui/app/src; restore them, or, if \
         they moved, update the consts xtask/src/checks/buildtime_edits.rs builds that place from",
    ]), inspects 0 => super::missing_src,
    /// An empty `ui/app/src` inspects nothing the same way.
    empty_src: FailsWith(&[
        "0 UI source files inspected under ui/app/src\n    \
         fix: buildtime-edits looks for UI source files under ui/app/src; restore them, or, if \
         they moved, update the consts xtask/src/checks/buildtime_edits.rs builds that place from",
    ]), inspects 0 => super::empty_src,
    /// Single quotes around the value fail the same way.
    single_quoted: FailsWith(&[
        "ui/app/src/App.tsx:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 1 => super::single_quoted,
    /// A braced JSX value fails the same way.
    braced_value: FailsWith(&[
        "ui/app/src/App.tsx:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 1 => super::braced_value,
    /// The attribute on its own line, as a formatter splits a tag, fails at that line.
    split_tag: FailsWith(&[
        "ui/app/src/App.tsx:3: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 1 => super::split_tag,
    /// Whitespace before the `=` does not hide the attribute.
    space_before_equals: FailsWith(&[
        "ui/app/src/App.tsx:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 1 => super::space_before_equals,
    /// The name and its `=` on two lines fail once, at the name's line.
    name_and_value_split: FailsWith(&[
        "ui/app/src/App.tsx:2: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 1 => super::name_and_value_split,
    /// CRLF line endings count lines the same way.
    crlf_file: FailsWith(&[
        "ui/app/src/App.tsx:2: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 1 => super::crlf_file,
    /// The attribute in a stylesheet fails: every file type is read, not only `.tsx`.
    stylesheet: FailsWith(&[
        "ui/app/src/app.css:4: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 2 => super::stylesheet,
    /// The attribute in a file under a hidden, nested folder fails: every folder is walked.
    hidden_deep_folder: FailsWith(&[
        "ui/app/src/.cache/a/b/state.json:1: an interrupted `cargo xtask buildtime web` left its edit here\n    \
         fix: run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, \
         or remove the attribute by hand",
    ]), inspects 2 => super::hidden_deep_folder,
    /// Names that only start like the attribute, or the bare name in a comment, pass.
    lookalikes_pass: Passes, inspects 1 => super::lookalikes_pass,
}
