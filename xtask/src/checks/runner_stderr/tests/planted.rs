//! The planted cases of the runner-stderr check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! `repos.rs`.

use crate::checks::planted::fails;
use crate::checks::runner_stderr::Fix;

crate::checks::planted::cases! {
    /// The runner's own code formats errors inside `Detail::new` and writes through the API.
    clean_runner: Passes, inspects 1 => super::repos::clean_runner,
    /// The API module, its submodules, the tests, the setup binary and the files outside the
    /// helper modules are not read.
    skipped_files: Passes, inspects 1 => super::repos::skipped_files,
    /// An inline `#[cfg(test)]` module is not read: its fake formats an error.
    inline_test_module: Passes, inspects 1 => super::repos::inline_test_module,
    /// `#[cfg(not(test))]` is read: its `eprintln!` fails at its line.
    cfg_not_test_module: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:5",
        "eprintln! writes to the console; the runner says things only through runner::stderr",
        Fix::Console,
    )], inspects 1 => super::repos::cfg_not_test_module,
    /// `eprintln!` in the runner fails at the macro's line.
    console_macro: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:3",
        "eprintln! writes to the console; the runner says things only through runner::stderr",
        Fix::Console,
    )], inspects 1 => super::repos::console_macro,
    /// `print!` in the binary fails too: the binary is read.
    print_in_bin: Fails[fails(
        "crates/sandbox/src/bin/clauseo-sandbox-run.rs:2",
        "print! writes to the console; the runner says things only through runner::stderr",
        Fix::Console,
    )], inspects 1 => super::repos::print_in_bin,
    /// `writeln!` to a stderr handle fails at the macro's line.
    write_to_stderr: Fails[fails(
        "crates/sandbox/src/windows/runner/inner.rs:2",
        "writeln! writes to a stderr handle; the runner says things only through runner::stderr",
        Fix::WriteStderr,
    )], inspects 1 => super::repos::write_to_stderr,
    /// `io::stderr().lock()` fails at the call's line.
    stderr_call: Fails[fails(
        "crates/sandbox/src/windows/runner/inner.rs:2",
        "stderr() opens stderr; the runner says things only through runner::stderr",
        Fix::StderrCall,
    )], inspects 1 => super::repos::stderr_call,
    /// A `stderr()` call inside a closure is still found.
    stderr_in_closure: Fails[fails(
        "crates/sandbox/src/windows/runner/inner.rs:2",
        "stderr() opens stderr; the runner says things only through runner::stderr",
        Fix::StderrCall,
    )], inspects 1 => super::repos::stderr_in_closure,
    /// An error named in a placeholder, outside `Detail::new`, fails at the literal.
    inline_error_text: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:2",
        "format! formats the error `error`; error text can hold a path, so format it inside Detail::new",
        Fix::ErrorText,
    )], inspects 1 => super::repos::inline_error_text,
    /// An error passed as a positional argument fails at the argument.
    positional_error_text: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:2",
        "format! formats the error `err`; error text can hold a path, so format it inside Detail::new",
        Fix::ErrorText,
    )], inspects 1 => super::repos::positional_error_text,
    /// `error.to_string()` outside `Detail::new` fails at the receiver.
    error_to_string: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:2",
        "`error.to_string()` formats the error; error text can hold a path, so format it inside Detail::new",
        Fix::ErrorText,
    )], inspects 1 => super::repos::error_to_string,
    /// `e.to_string()` as a format argument fails at the receiver.
    to_string_in_format: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:2",
        "`e.to_string()` formats the error; error text can hold a path, so format it inside Detail::new",
        Fix::ErrorText,
    )], inspects 1 => super::repos::to_string_in_format,
    /// A `use` that renames `Detail` fails where it is written.
    detail_alias: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:1",
        "`Detail` is renamed `D`, which runner-stderr can't follow",
        Fix::Alias,
    )], inspects 1 => super::repos::detail_alias,
    /// A `macro_rules!` whose body calls `eprintln!` fails where it is written.
    wrapping_macro: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:1",
        "a macro_rules! body calls eprintln!, which runner-stderr can't follow",
        Fix::Wrapper,
    )], inspects 1 => super::repos::wrapping_macro,
    /// `io::stderr` passed as a value, not called, fails at the path.
    stderr_as_value: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:2",
        "`stderr` taken as a value is stderr's handle; the runner says things only through runner::stderr",
        Fix::StderrValue,
    )], inspects 1 => super::repos::stderr_as_value,
    /// `io::stderr` stored in a binding fails at the path.
    stderr_stored: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:2",
        "`stderr` taken as a value is stderr's handle; the runner says things only through runner::stderr",
        Fix::StderrValue,
    )], inspects 1 => super::repos::stderr_stored,
    /// A `use` of `io::stderr` fails where it is written: a bare `stderr` would name it.
    stderr_imported: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:1",
        "this `use` brings io::stderr in under a bare name; the runner says things only through runner::stderr",
        Fix::StderrValue,
    )], inspects 1 => super::repos::stderr_imported,
    /// A helper that writes to the console fails; its error formatting passes.
    helper_console: Fails[fails(
        "crates/sandbox/src/windows/pipe.rs:2",
        "eprintln! writes to the console; the runner says things only through runner::stderr",
        Fix::Console,
    )], inspects 1 => super::repos::helper_console,
    /// A helper that hands `io::stderr` on fails at the path.
    helper_stderr_value: Fails[fails(
        "crates/sandbox/src/windows/token/os.rs:2",
        "`stderr` taken as a value is stderr's handle; the runner says things only through runner::stderr",
        Fix::StderrValue,
    )], inspects 1 => super::repos::helper_stderr_value,
    /// The files of a module declared under `#[cfg(test)]` are skipped; their parents are read.
    test_gated_helpers: Passes, inspects 2 => super::repos::test_gated_helpers,
    /// A `use crate::` of a module outside the helpers fails in the runner.
    outside_import: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:1",
        "`use crate::ports` imports from outside the helpers runner-stderr reads (crate::windows, crate::denial, crate::detect)",
        Fix::OutsideHelpers,
    )], inspects 1 => super::repos::outside_import,
    /// A name `lib.rs` re-exports from a helper module passes; one from another module fails.
    root_reexport: Fails[fails(
        "crates/sandbox/src/windows/runner/outer.rs:2",
        "`use crate::Port` imports from outside the helpers runner-stderr reads (crate::windows, crate::denial, crate::detect)",
        Fix::OutsideHelpers,
    )], inspects 1 => super::repos::root_reexport,
    /// A file syn cannot parse fails at the line it stopped on.
    unparsable: Fails[fails(
        "crates/sandbox/src/windows/runner/broken.rs:2",
        "syn cannot parse this file: expected an expression",
        Fix::Unparsable,
    )], inspects 1 => super::repos::unparsable,
}
