//! The planted cases of the file-size check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! `repos.rs`, and the lines several cases share in the check's tests module.

use crate::checks::file_size::Fix;
use crate::checks::planted::{fails, failure};

crate::checks::planted::cases! {
    /// A `.rs` file of 601 lines fails at line 1 and names the limit and the two fixes.
    rs_601_lines: Fails[fails(
        "src/big.rs:1",
        "this file has 601 lines; the limit is 600",
        Fix::SplitOrExempt,
    )], inspects 1 => super::repos::rs_601_lines,
    /// A `.ts` file of 601 lines fails the same way.
    ts_601_lines: Fails[fails(
        "ui/big.ts:1",
        "this file has 601 lines; the limit is 600",
        Fix::SplitOrExempt,
    )], inspects 1 => super::repos::ts_601_lines,
    /// A `.tsx` file of 601 lines fails the same way.
    tsx_601_lines: Fails[fails(
        "ui/Big.tsx:1",
        "this file has 601 lines; the limit is 600",
        Fix::SplitOrExempt,
    )], inspects 1 => super::repos::tsx_601_lines,
    /// A `.mts` file of 601 lines fails the same way, and so does an upper-case `.RS` file.
    mts_and_upper_case_rs_601_lines: Fails[
        fails(
            "src/LIB.RS:1",
            "this file has 601 lines; the limit is 600",
            Fix::SplitOrExempt,
        ),
        fails(
            "ui/big.mts:1",
            "this file has 601 lines; the limit is 600",
            Fix::SplitOrExempt,
        ),
    ], inspects 2 => super::repos::mts_and_upper_case_rs_601_lines,
    /// An exception for a path that no longer exists fails at its line in the exception file.
    stale_exception_gone: Fails[fails(
        "xtask/file-size-exceptions.txt:2",
        "`src/gone.rs` is listed as an exception, but the check inspects no Rust or TypeScript \
         file at that path",
        Fix::DeleteException,
    )], inspects 1 => super::repos::stale_exception_gone,
    /// An exception for a file of 450 lines fails: it needs no exception.
    stale_exception_short: Fails[fails(
        "xtask/file-size-exceptions.txt:2",
        "`src/short.rs` is listed as an exception, but it has 450 lines, within the 600-line limit",
        Fix::DeleteException,
    )], inspects 1 => super::repos::stale_exception_short,
    /// A line comment that reaches column 101 fails, and the message names the column.
    line_comment_101: Fails[fails(
        "src/lib.rs:2",
        "this comment reaches column 101; the limit is 100",
        Fix::WrapComment,
    )], inspects 1 => super::repos::line_comment_101,
    /// A doc comment (`//!`) that reaches column 101 fails.
    doc_comment_101: Fails[fails(
        "src/lib.rs:1",
        "this comment reaches column 101; the limit is 100",
        Fix::WrapComment,
    )], inspects 1 => super::repos::doc_comment_101,
    /// A comment after code whose last column is 102 fails, on the code's line.
    trailing_comment_past_100: Fails[fails(
        "src/lib.rs:1",
        "this comment reaches column 102; the limit is 100",
        Fix::WrapComment,
    )], inspects 1 => super::repos::trailing_comment_past_100,
    /// A block comment whose second physical line reaches past column 100 fails on that line.
    block_comment_past_100: Fails[fails(
        "src/lib.rs:2",
        "this comment reaches column 110; the limit is 100",
        Fix::WrapComment,
    )], inspects 1 => super::repos::block_comment_past_100,
    /// A comment of 100 characters, `//` included, whose rest is `é`, passes.
    e_acute_comment_100_passes: Passes, inspects 1 => super::repos::e_acute_comment_100_passes,
    /// A code line with a 101-character string literal passes: code is exempt.
    string_literal_101_passes: Passes, inspects 1 => super::repos::string_literal_101_passes,
    /// A 101-character string literal that holds `//` passes: it is not a comment.
    string_holding_slashes_101_passes: Passes, inspects 1
        => super::repos::string_holding_slashes_101_passes,
    /// A raw string (`r#"…"#`) holding `//`, on one line and across lines, passes.
    raw_string_holding_slashes_passes: Passes, inspects 1
        => super::repos::raw_string_holding_slashes_passes,
    /// A `'"'` char literal does not open a string, so the `//` in a later string is still
    /// inside a string and passes.
    char_quote_then_code_passes: Passes, inspects 1 => super::repos::char_quote_then_code_passes,
    /// Lifetimes and labels are not char literals, so the string after them passes.
    lifetime_then_string_passes: Passes, inspects 1 => super::repos::lifetime_then_string_passes,
    /// Outside a git repo the check says git's error, then that it stopped before it looked.
    not_a_git_repo: Fails[failure(
        "could not list the repo's files: `git ls-files -z --cached --others --exclude-standard` \
         failed: fatal: not a git repository (or any of the parent directories): .git",
        crate::checks::Fix::GitFailed {
            check: "file-size".to_owned(),
        },
    )], inspects 0 => super::repos::not_a_git_repo,
    /// A file that is not UTF-8 fails and is not counted; with no other file read, the check
    /// stopped on it.
    only_file_unreadable: Fails[fails(
        "bad.rs:1",
        "could not read `bad.rs`: stream did not contain valid UTF-8",
        crate::checks::Fix::NotUtf8,
    )], inspects 0 => super::repos::only_file_unreadable,
    /// A file of 650 lines listed in the exception file, with a reason, passes.
    listed_600_plus_file_passes: Passes, inspects 1 => super::repos::listed_600_plus_file_passes,
    /// A `.cts` file of 601 lines fails the same way.
    cts_601_lines: Fails[fails(
        "ui/big.cts:1",
        "this file has 601 lines; the limit is 600",
        Fix::SplitOrExempt,
    )], inspects 1 => super::repos::cts_601_lines,
    /// CRLF line endings count one line each.
    crlf_601_lines: Fails[fails(
        "src/big.rs:1",
        "this file has 601 lines; the limit is 600",
        Fix::SplitOrExempt,
    )], inspects 1 => super::repos::crlf_601_lines,
    /// Lone `\\r` line endings count one line each too.
    lone_cr_601_lines: Fails[fails(
        "ui/big.ts:1",
        "this file has 601 lines; the limit is 600",
        Fix::SplitOrExempt,
    )], inspects 1 => super::repos::lone_cr_601_lines,
    /// A `.rs` symlink to a long file of another kind is read through the link.
    symlinked_file_601_lines: Fails[fails(
        "src/link.rs:1",
        "this file has 601 lines; the limit is 600",
        Fix::SplitOrExempt,
    )], inspects 1 => super::repos::symlinked_file_601_lines,
    /// An outer doc comment (`///`) that reaches column 101 fails.
    outer_doc_comment_101: Fails[fails(
        "src/lib.rs:1",
        "this comment reaches column 101; the limit is 100",
        Fix::WrapComment,
    )], inspects 1 => super::repos::outer_doc_comment_101,
    /// A `/** */` doc comment whose second line reaches column 101 fails on that line.
    block_doc_comment_past_100: Fails[fails(
        "src/lib.rs:2",
        "this comment reaches column 101; the limit is 100",
        Fix::WrapComment,
    )], inspects 1 => super::repos::block_doc_comment_past_100,
    /// A `/*! */` doc comment whose first line reaches column 101 fails on that line.
    inner_block_doc_comment_past_100: Fails[fails(
        "src/lib.rs:1",
        "this comment reaches column 101; the limit is 100",
        Fix::WrapComment,
    )], inspects 1 => super::repos::inner_block_doc_comment_past_100,
    /// Text after a nested pair closes is still comment, and fails past column 100.
    nested_block_comment_past_100: Fails[fails(
        "src/lib.rs:2",
        "this comment reaches column 101; the limit is 100",
        Fix::WrapComment,
    )], inspects 1 => super::repos::nested_block_comment_past_100,
    /// A comment on a CRLF line that reaches column 101 fails; the `\\r` is not counted.
    crlf_comment_101: Fails[fails(
        "src/lib.rs:2",
        "this comment reaches column 101; the limit is 100",
        Fix::WrapComment,
    )], inspects 1 => super::repos::crlf_comment_101,
    /// A 100-column comment on a CRLF line passes.
    crlf_comment_100_passes: Passes, inspects 1 => super::repos::crlf_comment_100_passes,
    /// An exception for a file that is there but no source kind is stale.
    stale_exception_not_source: Fails[fails(
        "xtask/file-size-exceptions.txt:2",
        "`README.md` is listed as an exception, but the check inspects no Rust or TypeScript \
         file at that path",
        Fix::DeleteException,
    )], inspects 1 => super::repos::stale_exception_not_source,
    /// An exception with no reason fails at its line.
    exception_without_reason: Fails[fails(
        "xtask/file-size-exceptions.txt:2",
        "`src/lib.rs` has no reason",
        crate::checks::exceptions::Fix::WriteReason {
            path: "src/lib.rs".to_owned(),
        },
    )], inspects 1 => super::repos::exception_without_reason,
    /// An exception listed twice fails at the second line; the first still exempts its file.
    exception_listed_twice: Fails[fails(
        "xtask/file-size-exceptions.txt:3",
        "`src/parser.rs` is listed twice",
        crate::checks::exceptions::Fix::DeleteDuplicate,
    )], inspects 1 => super::repos::exception_listed_twice,
}
