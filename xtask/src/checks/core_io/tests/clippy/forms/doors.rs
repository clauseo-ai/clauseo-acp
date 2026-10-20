//! The doors of the root `clippy.toml`, which bans them in every crate: one must-fail form per
//! std call that changes files (the write module), both `canonicalize` paths (and `PathBuf`'s,
//! which reaches `Path::canonicalize` by deref), `env::split_paths` (the one program lookup), and
//! `Command::status` and `CommandExt::exec` (the process door); and must-pass forms for the
//! reads, which stay allowed. Each failing form plants its call on line 2 and must get a hit
//! there naming its entry, with the entry's reason or replacement, so deleting the entry, or
//! giving it another reason, fails the test.
//!
//! `std::os::windows::fs::symlink_file` and `symlink_dir` build on Windows only: the test checks
//! them where it runs on Windows.

use super::{Form, Must, Only, form};

/// What the root `clippy.toml` says about a write, as the hit's note carries it.
const WRITE_REASON: &str = "files change only through the crate's write module: \
                            xtask/src/write.rs in xtask";

/// The replacement the `canonicalize` entries name.
const DUNCE: &str = "dunce::canonicalize";

/// The process door's reason.
const PROCESS_REASON: &str = "children start only through xtask/src/process.rs";

/// The form `door_<name>`: `pub fn f(<params>)` whose line 2 is `let _ = <call>;`, which must
/// get a hit there naming `item`, with `note` in its reason or replacement.
fn planted(name: &str, only: Only, params: &str, call: &str, item: &str, note: &str) -> Form {
    form(
        &format!("door_{name}"),
        Must::Fail,
        only,
        &format!("pub fn f({params}) {{\n    let _ = {call};\n}}\n"),
    )
    .naming(item)
    .at(2)
    .noting(note)
}

/// One write the root file bans: the form's name, its targets, the parameters of `f`, the call,
/// and the entry it must hit.
type Write = (&'static str, Only, &'static str, &'static str, &'static str);

/// A `&File` parameter, for the calls on one.
const FILE: &str = "f: &std::fs::File";

/// Every std call that changes files, one per entry.
const WRITES: [Write; 24] = [
    (
        "fs_write",
        Only::Any,
        "",
        "std::fs::write(\"a\", \"b\")",
        "std::fs::write",
    ),
    (
        "fs_create_dir",
        Only::Any,
        "",
        "std::fs::create_dir(\"a\")",
        "std::fs::create_dir",
    ),
    (
        "fs_create_dir_all",
        Only::Any,
        "",
        "std::fs::create_dir_all(\"a\")",
        "std::fs::create_dir_all",
    ),
    (
        "fs_remove_file",
        Only::Any,
        "",
        "std::fs::remove_file(\"a\")",
        "std::fs::remove_file",
    ),
    (
        "fs_remove_dir",
        Only::Any,
        "",
        "std::fs::remove_dir(\"a\")",
        "std::fs::remove_dir",
    ),
    (
        "fs_remove_dir_all",
        Only::Any,
        "",
        "std::fs::remove_dir_all(\"a\")",
        "std::fs::remove_dir_all",
    ),
    (
        "fs_rename",
        Only::Any,
        "",
        "std::fs::rename(\"a\", \"b\")",
        "std::fs::rename",
    ),
    (
        "fs_copy",
        Only::Any,
        "",
        "std::fs::copy(\"a\", \"b\")",
        "std::fs::copy",
    ),
    (
        "fs_hard_link",
        Only::Any,
        "",
        "std::fs::hard_link(\"a\", \"b\")",
        "std::fs::hard_link",
    ),
    (
        "fs_soft_link",
        Only::Any,
        "",
        "std::fs::soft_link(\"a\", \"b\")",
        "std::fs::soft_link",
    ),
    (
        "fs_set_permissions",
        Only::Any,
        "p: std::fs::Permissions",
        "std::fs::set_permissions(\"a\", p)",
        "std::fs::set_permissions",
    ),
    (
        "file_create",
        Only::Any,
        "",
        "std::fs::File::create(\"a\")",
        "std::fs::File::create",
    ),
    (
        "file_create_new",
        Only::Any,
        "",
        "std::fs::File::create_new(\"a\")",
        "std::fs::File::create_new",
    ),
    (
        "file_set_len",
        Only::Any,
        FILE,
        "f.set_len(0)",
        "std::fs::File::set_len",
    ),
    (
        "file_set_permissions",
        Only::Any,
        "f: &std::fs::File, p: std::fs::Permissions",
        "f.set_permissions(p)",
        "std::fs::File::set_permissions",
    ),
    (
        "file_set_times",
        Only::Any,
        FILE,
        "f.set_times(std::fs::FileTimes::new())",
        "std::fs::File::set_times",
    ),
    (
        "file_set_modified",
        Only::Any,
        FILE,
        "f.set_modified(std::time::SystemTime::UNIX_EPOCH)",
        "std::fs::File::set_modified",
    ),
    // Banned whatever its options: a read never needs OpenOptions.
    (
        "open_options_open",
        Only::Any,
        "",
        "std::fs::OpenOptions::new().read(true).open(\"a\")",
        "std::fs::OpenOptions::open",
    ),
    (
        "dir_builder_create",
        Only::Any,
        "",
        "std::fs::DirBuilder::new().create(\"a\")",
        "std::fs::DirBuilder::create",
    ),
    (
        "unix_symlink",
        Only::Unix,
        "",
        "std::os::unix::fs::symlink(\"a\", \"b\")",
        "std::os::unix::fs::symlink",
    ),
    (
        "unix_chown",
        Only::Unix,
        "",
        "std::os::unix::fs::chown(\"a\", None, None)",
        "std::os::unix::fs::chown",
    ),
    (
        "unix_lchown",
        Only::Unix,
        "",
        "std::os::unix::fs::lchown(\"a\", None, None)",
        "std::os::unix::fs::lchown",
    ),
    (
        "windows_symlink_file",
        Only::Windows,
        "",
        "std::os::windows::fs::symlink_file(\"a\", \"b\")",
        "std::os::windows::fs::symlink_file",
    ),
    (
        "windows_symlink_dir",
        Only::Windows,
        "",
        "std::os::windows::fs::symlink_dir(\"a\", \"b\")",
        "std::os::windows::fs::symlink_dir",
    ),
];

/// The must-fail forms: every path the root file bans for the write, path and process doors.
pub(super) fn failing() -> Vec<Form> {
    let mut all: Vec<Form> = WRITES
        .iter()
        .map(|&(name, only, params, call, item)| {
            planted(name, only, params, call, item, WRITE_REASON)
        })
        .collect();
    all.extend([
        planted(
            "fs_canonicalize",
            Only::Any,
            "",
            "std::fs::canonicalize(\"a\")",
            "std::fs::canonicalize",
            DUNCE,
        ),
        planted(
            "path_canonicalize",
            Only::Any,
            "",
            "std::path::Path::new(\"a\").canonicalize()",
            "std::path::Path::canonicalize",
            DUNCE,
        ),
        planted(
            "path_buf_canonicalize",
            Only::Any,
            "",
            "std::path::PathBuf::from(\"a\").canonicalize()",
            "std::path::Path::canonicalize",
            DUNCE,
        ),
        planted(
            "split_paths",
            Only::Any,
            "",
            "std::env::split_paths(\"a:b\").count()",
            "std::env::split_paths",
            "only resolve_pnpm in xtask/src/process.rs looks programs up on PATH",
        ),
        planted(
            "command_status",
            Only::Any,
            "",
            "std::process::Command::new(\"true\").status()",
            "std::process::Command::status",
            PROCESS_REASON,
        ),
        planted(
            "command_ext_exec",
            Only::Unix,
            "",
            "std::os::unix::process::CommandExt::exec(&mut std::process::Command::new(\"true\"))",
            "std::os::unix::process::CommandExt::exec",
            PROCESS_REASON,
        ),
    ]);
    all
}

/// The must-pass forms: the reads the doors leave allowed. Only the root file's crate builds
/// them, since `xtask/core-io-bans.toml` bans reads too.
pub(super) fn passing() -> Vec<Form> {
    let read = |name: &str, body: &str| {
        form(
            &format!("door_{name}"),
            Must::Pass,
            Only::Any,
            &format!("pub fn f() {{\n    {body}\n}}\n"),
        )
    };
    vec![
        read("read_to_string", "let _ = std::fs::read_to_string(\"a\");"),
        read("read", "let _ = std::fs::read(\"a\");"),
        read("read_dir", "let _ = std::fs::read_dir(\"a\");"),
        read("metadata", "let _ = std::fs::metadata(\"a\");"),
        read("file_open", "let _ = std::fs::File::open(\"a\");"),
        // Dropping a File opened for reading changes nothing; reading from it is no write.
        read(
            "file_open_and_read",
            "use std::io::Read;\n    let mut text = String::new();\n    \
             let _ = std::fs::File::open(\"a\").and_then(|mut f| f.read_to_string(&mut text));",
        ),
    ]
}
