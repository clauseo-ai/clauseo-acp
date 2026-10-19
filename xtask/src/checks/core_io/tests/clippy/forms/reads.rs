//! One must-fail form per entry the list holds beyond the modules it bans whole: the disk,
//! clock and stream reads, and the trait methods and functions that do I/O without a value of a
//! banned type. Each form's hit must name its entry, so deleting the entry fails the test.

use super::{Form, Must, Only, fail, form};

pub(super) fn forms() -> Vec<Form> {
    let mut all = path_reads();
    all.extend(others());
    all
}

/// Each `Path` read, called through a `PathBuf` (by deref) or a `&Path`, and `path::absolute`.
fn path_reads() -> Vec<Form> {
    let on_path_buf = |method: &str| {
        fail(
            &format!("read_path_{method}"),
            &format!("pub fn f(p: &std::path::PathBuf) {{\n    let _ = p.{method}();\n}}\n"),
        )
        .naming(&format!("std::path::Path::{method}"))
    };
    let mut all: Vec<Form> = [
        "exists",
        "try_exists",
        "metadata",
        "symlink_metadata",
        "canonicalize",
        "read_dir",
        "read_link",
        "is_file",
        "is_dir",
        "is_symlink",
    ]
    .into_iter()
    .map(on_path_buf)
    .collect();
    all.push(
        fail(
            "read_path_on_a_path",
            "pub fn f() -> bool {\n    std::path::Path::new(\"a\").exists()\n}\n",
        )
        .naming("std::path::Path::exists"),
    );
    all.push(
        fail(
            "read_path_absolute",
            "pub fn f() {\n    let _ = std::path::absolute(\"a\");\n}\n",
        )
        .naming("std::path::absolute"),
    );
    all
}

/// The clock and stream reads, and the trait methods and functions outside the banned modules.
fn others() -> Vec<Form> {
    vec![
        fail(
            "read_instant_elapsed",
            "pub fn f(start: std::time::Instant) -> std::time::Duration {\n    \
             start.elapsed()\n}\n",
        )
        .naming("std::time::Instant::elapsed"),
        fail(
            "read_system_time_elapsed",
            "pub fn f() {\n    let _ = std::time::UNIX_EPOCH.elapsed();\n}\n",
        )
        .naming("std::time::SystemTime::elapsed"),
        fail(
            "read_stdin",
            "use std::io::BufRead;\npub fn f() -> usize {\n    \
             std::io::stdin().lock().lines().count()\n}\n",
        )
        .naming("std::io::stdin"),
        fail(
            "read_stdout",
            "use std::io::Write;\npub fn f() {\n    let _ = std::io::stdout().flush();\n}\n",
        )
        .naming("std::io::stdout"),
        fail(
            "read_stderr",
            "use std::io::Write;\npub fn f() {\n    let _ = std::io::stderr().flush();\n}\n",
        )
        .naming("std::io::stderr"),
        fail(
            "read_to_socket_addrs_through_a_glob",
            "use std::net::*;\npub fn f() -> bool {\n    \
             \"localhost:80\".to_socket_addrs().is_ok()\n}\n",
        )
        .naming("std::net::ToSocketAddrs::to_socket_addrs"),
        fail(
            "read_available_parallelism",
            "pub fn f() -> usize {\n    \
             std::thread::available_parallelism().map_or(1, std::num::NonZero::get)\n}\n",
        )
        .naming("std::thread::available_parallelism"),
        form(
            "read_is_terminal_of_an_fd",
            Must::Fail,
            Only::Unix,
            "use std::io::IsTerminal;\npub fn f(fd: std::os::fd::BorrowedFd<'_>) -> bool {\n    \
             fd.is_terminal()\n}\n",
        )
        .naming("std::io::IsTerminal::is_terminal"),
        fail(
            "read_termination_report_through_a_glob",
            "use std::process::*;\npub fn f(r: Result<(), String>) {\n    let _ = r.report();\n}\n",
        )
        .naming("std::process::Termination::report"),
        fail(
            "read_backtrace_capture",
            "pub fn f() -> String {\n    std::backtrace::Backtrace::capture().to_string()\n}\n",
        )
        .naming("std::backtrace::Backtrace::capture"),
        fail(
            "read_backtrace_force_capture",
            "pub fn f() -> String {\n    \
             std::backtrace::Backtrace::force_capture().to_string()\n}\n",
        )
        .naming("std::backtrace::Backtrace::force_capture"),
    ]
}
