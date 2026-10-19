//! The planted cases of the file-size check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A `.rs` file of 601 lines fails at line 1 and names the limit and the two fixes.
    rs_601_lines: FailsWith(&[
        "src/big.rs:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
    ]) => super::rs_601_lines,
    /// A `.ts` file of 601 lines fails the same way.
    ts_601_lines: FailsWith(&[
        "ui/big.ts:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
    ]) => super::ts_601_lines,
    /// A `.tsx` file of 601 lines fails the same way.
    tsx_601_lines: FailsWith(&[
        "ui/Big.tsx:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
    ]) => super::tsx_601_lines,
    /// A `.mts` file of 601 lines fails the same way, and so does an upper-case `.RS` file.
    mts_and_upper_case_rs_601_lines: FailsWith(&[
        "src/LIB.RS:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
        "ui/big.mts:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
    ]) => super::mts_and_upper_case_rs_601_lines,
    /// An exception for a path that no longer exists fails at its line in the exception file.
    stale_exception_gone: FailsWith(&[
        "xtask/file-size-exceptions.txt:2: `src/gone.rs` is listed as an exception, but the \
         check inspects no Rust or TypeScript file at that path\n    \
         fix: delete this line from xtask/file-size-exceptions.txt",
    ]) => super::stale_exception_gone,
    /// An exception for a file of 450 lines fails: it needs no exception.
    stale_exception_short: FailsWith(&[
        "xtask/file-size-exceptions.txt:2: `src/short.rs` is listed as an exception, but it has \
         450 lines, within the 600-line limit\n    \
         fix: delete this line from xtask/file-size-exceptions.txt",
    ]) => super::stale_exception_short,
    /// A line comment that reaches column 101 fails, and the message names the column.
    line_comment_101: FailsWith(&[
        "src/lib.rs:2: this comment reaches column 101; the limit is 100\n    \
         fix: wrap the comment at 100 columns",
    ]) => super::line_comment_101,
    /// A doc comment (`//!`) that reaches column 101 fails.
    doc_comment_101: FailsWith(&[
        "src/lib.rs:1: this comment reaches column 101; the limit is 100\n    \
         fix: wrap the comment at 100 columns",
    ]) => super::doc_comment_101,
    /// A comment after code whose last column is 102 fails, on the code's line.
    trailing_comment_past_100: FailsWith(&[
        "src/lib.rs:1: this comment reaches column 102; the limit is 100\n    \
         fix: wrap the comment at 100 columns",
    ]) => super::trailing_comment_past_100,
    /// A block comment whose second physical line reaches past column 100 fails on that line.
    block_comment_past_100: FailsWith(&[
        "src/lib.rs:2: this comment reaches column 110; the limit is 100\n    \
         fix: wrap the comment at 100 columns",
    ]) => super::block_comment_past_100,
    /// A comment of 100 characters, `//` included, whose rest is `é`, passes.
    e_acute_comment_100_passes: Passes => super::e_acute_comment_100_passes,
    /// A code line with a 101-character string literal passes: code is exempt.
    string_literal_101_passes: Passes => super::string_literal_101_passes,
    /// A 101-character string literal that holds `//` passes: it is not a comment.
    string_holding_slashes_101_passes: Passes => super::string_holding_slashes_101_passes,
    /// A raw string (`r#"…"#`) holding `//`, on one line and across lines, passes.
    raw_string_holding_slashes_passes: Passes => super::raw_string_holding_slashes_passes,
    /// A `'"'` char literal does not open a string, so the `//` in a later string is still
    /// inside a string and passes.
    char_quote_then_code_passes: Passes => super::char_quote_then_code_passes,
    /// Lifetimes and labels are not char literals, so the string after them passes.
    lifetime_then_string_passes: Passes => super::lifetime_then_string_passes,
    /// A file of 650 lines listed in the exception file, with a reason, passes.
    listed_600_plus_file_passes: Passes => super::listed_600_plus_file_passes,
}
