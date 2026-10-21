//! The doors of the root `clippy.toml`, which bans them in every crate: one must-fail form per
//! std call that changes files (the write module), both `canonicalize` paths (and `PathBuf`'s,
//! which reaches `Path::canonicalize` by deref), `env::split_paths` (the one walk over PATH), and
//! `Command::status`, `CommandExt::exec` and nix's safe spawn and exec calls (the process door),
//! and the clock, `thread::sleep`, `Command::output` and `spawn` and `serde_json::from_reader`;
//! and must-pass forms for the reads, which stay allowed. The tauri entries have none
//! (`NOT_BUILT`). Each failing form plants its call on line 2 and must get a hit
//! there naming its entry, with the entry's reason or replacement, so deleting the entry, or
//! giving it another reason, fails the test.
//!
//! `std::os::windows::fs::symlink_file` and `symlink_dir` build on Windows only, and nix's
//! `execvpe`, `fexecve` and `execveat` on Linux only: the test checks each where it builds.

use super::{Form, Must, Only, form};

/// What the root `clippy.toml` says about a write, as the hit's note carries it.
const WRITE_REASON: &str = "files change only through the crate's write module; see AGENTS.md";

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
const WRITES: [Write; 31] = [
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
        "fs_set_times",
        Only::Any,
        "",
        "std::fs::set_times(\"a\", std::fs::FileTimes::new())",
        "std::fs::set_times",
    ),
    (
        "fs_set_times_nofollow",
        Only::Any,
        "",
        "std::fs::set_times_nofollow(\"a\", std::fs::FileTimes::new())",
        "std::fs::set_times_nofollow",
    ),
    (
        "unix_fchown",
        Only::Unix,
        FILE,
        "std::os::unix::fs::fchown(f, None, None)",
        "std::os::unix::fs::fchown",
    ),
    // A bind creates the socket file at its path.
    (
        "unix_listener_bind",
        Only::Unix,
        "",
        "std::os::unix::net::UnixListener::bind(\"s\")",
        "std::os::unix::net::UnixListener::bind",
    ),
    (
        "unix_listener_bind_addr",
        Only::Unix,
        "a: &std::os::unix::net::SocketAddr",
        "std::os::unix::net::UnixListener::bind_addr(a)",
        "std::os::unix::net::UnixListener::bind_addr",
    ),
    (
        "unix_datagram_bind",
        Only::Unix,
        "",
        "std::os::unix::net::UnixDatagram::bind(\"s\")",
        "std::os::unix::net::UnixDatagram::bind",
    ),
    (
        "unix_datagram_bind_addr",
        Only::Unix,
        "a: &std::os::unix::net::SocketAddr",
        "std::os::unix::net::UnixDatagram::bind_addr(a)",
        "std::os::unix::net::UnixDatagram::bind_addr",
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

/// The entries of the root file in crates the scratch crates do not build: `tauri` and
/// `tauri_build` take minutes to build cold, too long for a test. `every_root_entry_has_a_form`
/// pins this list exactly, so an entry without a form fails unless it is added here, in review.
pub(crate) const NOT_BUILT: [&str; 14] = [
    "tauri::Builder::plugin",
    "tauri::Builder::plugin_boxed",
    "tauri::AppHandle::plugin",
    "tauri::AppHandle::plugin_boxed",
    "tauri::Manager::add_capability",
    "tauri::Context::runtime_authority_mut",
    "tauri::ipc::RuntimeAuthority::add_capability",
    "tauri::Context::config_mut",
    "tauri::Context::new",
    "tauri::Context::set_assets",
    "tauri_build::Attributes::capabilities_path_pattern",
    "tauri_build::AppManifest::permissions_path_pattern",
    "tauri_build::InlinedPlugin::permissions_path_pattern",
    "tauri::ipc::RuntimeAuthority::__allow_command",
];

/// The process crate's reason, for `Command::output` and `Command::spawn`.
const LAUNCH_REASON: &str = "agents and tools are launched by the `process` crate";

/// The clock entries' reason.
const CLOCK_REASON: &str = "the core takes time as input";

/// The parameters of `f` for the nix spawn calls.
const SPAWN: &str = "fa: &nix::spawn::PosixSpawnFileActions, at: &nix::spawn::PosixSpawnAttr";

/// A file descriptor parameter, for the nix exec calls on one.
const FD: &str = "fd: std::os::fd::BorrowedFd<'_>";

/// nix's safe calls that start or become another program: the form's name, its targets, the
/// parameters of `f`, the call, and the entry it must hit.
const NIX: [Write; 8] = [
    (
        "nix_posix_spawn",
        Only::Unix,
        SPAWN,
        "nix::spawn::posix_spawn(c\"a\", fa, at, &[c\"a\"], &[c\"a\"])",
        "nix::spawn::posix_spawn",
    ),
    (
        "nix_posix_spawnp",
        Only::Unix,
        SPAWN,
        "nix::spawn::posix_spawnp(c\"a\", fa, at, &[c\"a\"], &[c\"a\"])",
        "nix::spawn::posix_spawnp",
    ),
    (
        "nix_execv",
        Only::Unix,
        "",
        "nix::unistd::execv(c\"a\", &[c\"a\"])",
        "nix::unistd::execv",
    ),
    (
        "nix_execve",
        Only::Unix,
        "",
        "nix::unistd::execve(c\"a\", &[c\"a\"], &[c\"a\"])",
        "nix::unistd::execve",
    ),
    (
        "nix_execvp",
        Only::Unix,
        "",
        "nix::unistd::execvp(c\"a\", &[c\"a\"])",
        "nix::unistd::execvp",
    ),
    (
        "nix_execvpe",
        Only::Linux,
        "",
        "nix::unistd::execvpe(c\"a\", &[c\"a\"], &[c\"a\"])",
        "nix::unistd::execvpe",
    ),
    (
        "nix_fexecve",
        Only::Linux,
        FD,
        "nix::unistd::fexecve(fd, &[c\"a\"], &[c\"a\"])",
        "nix::unistd::fexecve",
    ),
    (
        "nix_execveat",
        Only::Linux,
        FD,
        "nix::unistd::execveat(fd, c\"a\", &[c\"a\"], &[c\"a\"], nix::fcntl::AtFlags::empty())",
        "nix::unistd::execveat",
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
            "thread_sleep",
            Only::Any,
            "",
            "std::thread::sleep(std::time::Duration::ZERO)",
            "std::thread::sleep",
            "blocks; tests use an injected clock",
        ),
        planted(
            "command_output",
            Only::Any,
            "",
            "std::process::Command::new(\"true\").output()",
            "std::process::Command::output",
            LAUNCH_REASON,
        ),
        planted(
            "command_spawn",
            Only::Any,
            "",
            "std::process::Command::new(\"true\").spawn()",
            "std::process::Command::spawn",
            LAUNCH_REASON,
        ),
        planted(
            "instant_now",
            Only::Any,
            "",
            "std::time::Instant::now()",
            "std::time::Instant::now",
            CLOCK_REASON,
        ),
        planted(
            "system_time_now",
            Only::Any,
            "",
            "std::time::SystemTime::now()",
            "std::time::SystemTime::now",
            CLOCK_REASON,
        ),
        planted(
            "serde_json_from_reader",
            Only::Any,
            "",
            "serde_json::from_reader::<_, u8>(std::io::empty())",
            "serde_json::from_reader",
            "serde_json::from_slice",
        ),
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
            "only first_on_path in xtask/src/process.rs walks PATH",
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
    all.extend(NIX.iter().map(|&(name, only, params, call, item)| {
        planted(name, only, params, call, item, PROCESS_REASON)
    }));
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
