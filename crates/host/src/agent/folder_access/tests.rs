use std::io;
use std::path::Path;

use clauseo_protocol::ErrorCode;

use super::{open_folder, preflight, session_refusal};
use crate::error::Error;

/// `EPERM` and `EACCES`, as macOS and Linux number them (Windows numbers its codes otherwise).
#[cfg(unix)]
const EPERM: i32 = 1;
#[cfg(unix)]
const EACCES: i32 = 13;

/// The matter folder the tests name; nothing opens it.
fn folder() -> &'static Path {
    Path::new("/Users/lawyer/Documents/State v. Desai")
}

/// The refusal's code and message, or `None` when `result` let the launch go on.
fn refused(result: Result<(), Error>) -> Option<(ErrorCode, String)> {
    result
        .err()
        .map(|error| (error.code(), error.to_wire().message().to_owned()))
}

#[cfg(unix)]
#[test]
fn an_open_refused_with_eperm_refuses_the_launch_with_folder_permission_and_no_path() {
    let (code, message) = refused(preflight(folder(), |_| {
        Err(io::Error::from_raw_os_error(EPERM))
    }))
    .expect("a refusal");
    assert_eq!(code, ErrorCode::MatterFolderPermission);
    assert!(!message.contains("Documents"), "{message}");
}

#[cfg(unix)]
#[test]
fn an_open_refused_with_eacces_refuses_the_launch_too() {
    let found = refused(preflight(folder(), |_| {
        Err(io::Error::from_raw_os_error(EACCES))
    }));
    assert_eq!(
        found.map(|(code, _)| code),
        Some(ErrorCode::MatterFolderPermission)
    );
}

#[test]
fn an_open_refused_permission_on_any_os_refuses_the_launch() {
    let found = refused(preflight(folder(), |_| {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }));
    assert_eq!(
        found.map(|(code, _)| code),
        Some(ErrorCode::MatterFolderPermission)
    );
}

#[test]
fn the_check_opens_the_matter_folder_it_is_given() {
    let mut opened = None;
    let result = preflight(folder(), |path| {
        opened = Some(path.to_path_buf());
        Ok(())
    });
    assert!(result.is_ok());
    assert_eq!(opened.as_deref(), Some(folder()));
}

#[test]
fn any_other_failure_lets_the_launch_go_on_for_the_other_checks() {
    for kind in [
        io::ErrorKind::NotFound,
        io::ErrorKind::NotADirectory,
        io::ErrorKind::TimedOut,
        io::ErrorKind::Other,
    ] {
        let result = preflight(folder(), |_| Err(io::Error::from(kind)));
        assert!(result.is_ok(), "{kind:?}");
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn off_macos_nothing_is_opened_so_nothing_is_refused() {
    assert!(open_folder(Path::new("/nonexistent/clauseo-folder-access")).is_ok());
}

#[cfg(target_os = "macos")]
#[test]
fn on_macos_the_open_reaches_the_file_system() {
    let error = open_folder(Path::new("/nonexistent/clauseo-folder-access"))
        .expect_err("a folder that is not there does not open");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

/// An error answer to `method`, as the ACP crate gives it, its `data` as a shape.
fn rpc(method: &str, code: i32, message: &str, data: &str) -> Error {
    Error::Acp(clauseo_acp::Error::Rpc {
        method: method.to_owned(),
        code,
        message: message.to_owned(),
        data: data.to_owned(),
        data_text: None,
        data_fields: clauseo_acp::RpcData::default(),
    })
}

/// Claude's adapter's answer, as `claude-agent-acp` 0.89.1 writes it (`validateCwd`, through
/// the SDK's `RequestError.invalidParams({ cwd }, ...)`).
fn cwd_missing() -> Error {
    let message = format!(
        "Invalid params: `cwd` does not exist on the machine running the agent: {}",
        folder().display()
    );
    rpc("session/new", -32602, &message, "keys [cwd]")
}

#[test]
fn claudes_cwd_answer_is_folder_permission_with_no_path() {
    let error = session_refusal(&cwd_missing()).expect("a refusal");
    assert_eq!(error.code(), ErrorCode::MatterFolderPermission);
    let message = error.to_wire().message().to_owned();
    assert!(!message.contains("Documents"), "{message}");
}

#[test]
fn other_launch_failures_keep_their_code() {
    let missing = "Invalid params: `cwd` does not exist on the machine running the agent: /tmp/x";
    let others = [
        // The same answer to another request.
        rpc("initialize", -32602, missing, "keys [cwd]"),
        rpc("session/load", -32602, missing, "keys [cwd]"),
        // Under another code.
        rpc("session/new", -32603, missing, "keys [cwd]"),
        // With no `cwd` in its data.
        rpc("session/new", -32602, missing, "none"),
        rpc("session/new", -32602, missing, "keys [path]"),
        rpc("session/new", -32602, missing, "string"),
        // Another invalid `cwd`: not a folder, or not absolute.
        rpc(
            "session/new",
            -32602,
            "Invalid params: `cwd` is not a directory: /tmp/x",
            "keys [cwd]",
        ),
        rpc(
            "session/new",
            -32602,
            "Invalid params: `cwd` must be an absolute path, but received: x",
            "keys [cwd]",
        ),
        Error::Acp(clauseo_acp::Error::Protocol("a line over 8 MiB".to_owned())),
        Error::Acp(clauseo_acp::Error::Closed),
        Error::Internal("x".to_owned()),
    ];
    for other in others {
        assert!(session_refusal(&other).is_none(), "{other:?}");
    }
}

#[test]
fn a_cwd_key_among_others_still_counts() {
    let message = "Invalid params: `cwd` does not exist on the machine running the agent: /x";
    let error = rpc("session/new", -32602, message, "keys [reason, cwd]");
    assert_eq!(
        session_refusal(&error).map(|error| error.code()),
        Some(ErrorCode::MatterFolderPermission)
    );
}
