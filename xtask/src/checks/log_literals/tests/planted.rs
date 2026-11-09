//! The planted cases of the log-literals check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! `repos.rs`.

use crate::checks::log_literals::Fix;
use crate::checks::planted::fails;

crate::checks::planted::cases! {
    /// Literal messages, with fields or none, pass.
    literal_messages: Passes, inspects 1 => super::repos::literal_messages,
    /// A name in the literal fails at the call's line.
    inline_argument: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:3",
        "warn! formats its message; runner.log keeps only literals",
        Fix::Literal,
    )], inspects 1 => super::repos::inline_argument,
    /// An argument after the literal fails too, through any path to the macro.
    trailing_argument: Fails[fails(
        "crates/sandbox/src/windows/apply.rs:2",
        "error! formats its message; runner.log keeps only literals",
        Fix::Literal,
    )], inspects 1 => super::repos::trailing_argument,
    /// A call inside a closure or a nested macro's body that syn parses is still found.
    nested_call: Fails[fails(
        "crates/sandbox/src/windows/ledger.rs:2",
        "warn! formats its message; runner.log keeps only literals",
        Fix::Literal,
    )], inspects 1 => super::repos::nested_call,
    /// Tests, the setup binary's folder and other folders are not read: only the one clean file.
    skipped_files: Passes, inspects 1 => super::repos::skipped_files,
    /// A file syn cannot parse fails at the line it stopped on.
    unparsable: Fails[fails(
        "crates/sandbox/src/windows/broken.rs:2",
        "syn cannot parse this file: expected an expression",
        Fix::Unparsable,
    )], inspects 1 => super::repos::unparsable,
}
