//! The planted cases of the buildtime-edits check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). Their builders are in `tests/repos.rs`.

use crate::checks::buildtime_edits::Fix;
use crate::checks::planted::fails;

crate::checks::planted::cases! {
    /// A clean `ui/app/src`, with a nested file and no attribute, passes.
    clean_src_passes: Passes, inspects 2 => super::repos::clean_src_passes,
    /// An attribute left in `App.tsx` with the lock free fails at its line, worded as a leftover.
    left_attribute: Fails[fails(
        "ui/app/src/App.tsx:1",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 1 => super::repos::left_attribute,
    /// Two lines of one file that hold the attribute fail one at a time.
    two_lines_in_one_file: Fails[
        fails(
            "ui/app/src/App.tsx:1",
            "an interrupted `cargo xtask buildtime web` left its edit here",
            Fix::Leftover,
        ),
        fails(
            "ui/app/src/App.tsx:3",
            "an interrupted `cargo xtask buildtime web` left its edit here",
            Fix::Leftover,
        ),
    ], inspects 1 => super::repos::two_lines_in_one_file,
    /// The attribute in a file nested under `ui/app/src/views/` fails at its line.
    nested_file: Fails[fails(
        "ui/app/src/views/V.tsx:2",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 2 => super::repos::nested_file,
    /// The attribute in a file that is not UTF-8 still fails: the bytes are read lossily.
    non_utf8_file: Fails[fails(
        "ui/app/src/logo.bin:1",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 1 => super::repos::non_utf8_file,
    /// The attribute while the buildtime lock is held is a live run, not a leftover.
    live_run_holds_the_edit: Fails[fails(
        "ui/app/src/App.tsx:1",
        "a `cargo xtask buildtime` run is in progress and holds this edit",
        Fix::LiveRun,
    )], inspects 1 => super::repos::live_run_holds_the_edit,
    /// A `ui/app/src` that is missing inspects nothing, and the zero line names the fix.
    missing_src: Fails[super::nothing_inspected()], inspects 0 => super::repos::missing_src,
    /// An empty `ui/app/src` inspects nothing the same way.
    empty_src: Fails[super::nothing_inspected()], inspects 0 => super::repos::empty_src,
    /// Single quotes around the value fail the same way.
    single_quoted: Fails[fails(
        "ui/app/src/App.tsx:1",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 1 => super::repos::single_quoted,
    /// A braced JSX value fails the same way.
    braced_value: Fails[fails(
        "ui/app/src/App.tsx:1",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 1 => super::repos::braced_value,
    /// The attribute on its own line, as a formatter splits a tag, fails at that line.
    split_tag: Fails[fails(
        "ui/app/src/App.tsx:3",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 1 => super::repos::split_tag,
    /// Whitespace before the `=` does not hide the attribute.
    space_before_equals: Fails[fails(
        "ui/app/src/App.tsx:1",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 1 => super::repos::space_before_equals,
    /// The name and its `=` on two lines fail once, at the name's line.
    name_and_value_split: Fails[fails(
        "ui/app/src/App.tsx:2",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 1 => super::repos::name_and_value_split,
    /// CRLF line endings count lines the same way.
    crlf_file: Fails[fails(
        "ui/app/src/App.tsx:2",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 1 => super::repos::crlf_file,
    /// The attribute in a stylesheet fails: every file type is read, not only `.tsx`.
    stylesheet: Fails[fails(
        "ui/app/src/app.css:4",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 2 => super::repos::stylesheet,
    /// The attribute in a file under a hidden, nested folder fails: every folder is walked.
    hidden_deep_folder: Fails[fails(
        "ui/app/src/.cache/a/b/state.json:1",
        "an interrupted `cargo xtask buildtime web` left its edit here",
        Fix::Leftover,
    )], inspects 2 => super::repos::hidden_deep_folder,
    /// Names that only start like the attribute, or the bare name in a comment, pass.
    lookalikes_pass: Passes, inspects 1 => super::repos::lookalikes_pass,
}
