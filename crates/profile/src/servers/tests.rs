use std::path::{Path, PathBuf};

use clauseo_settings::Locations;

use super::browser_for;
use crate::{BrowserSidecars, ProfilePaths};

fn paths() -> ProfilePaths {
    let abs = |path: &str| {
        if cfg!(windows) {
            PathBuf::from(format!("C:{path}"))
        } else {
            PathBuf::from(path)
        }
    };
    ProfilePaths::new(
        &Locations::under_root(Path::new("/data")),
        abs("/app/clauseo"),
        abs("/app/stetdoc"),
        BrowserSidecars {
            clauseo_mcp: abs("/app/clauseo-mcp"),
            agent_browser: abs("/app/agent-browser"),
        },
    )
    .unwrap()
}

/// The guard's ports: `--cdp-port` on both, `--daemon-port` on Windows only, and the stream
/// port; the socket folder on macOS and Linux only.
#[test]
fn each_os_gets_its_own_port_and_socket_tokens() {
    let unix = browser_for(&paths(), false);
    let windows = browser_for(&paths(), true);
    let after_cwd = |args: &[String]| args[4..].to_vec();
    assert_eq!(
        after_cwd(&unix.args),
        [
            "{browser_dir}",
            "--cdp-port",
            "{cdp_port}",
            "--",
            "mcp",
            "--tools",
            "core"
        ]
    );
    assert_eq!(
        after_cwd(&windows.args),
        [
            "{browser_dir}",
            "--cdp-port",
            "{cdp_port}",
            "--daemon-port",
            "{daemon_port}",
            "--",
            "mcp",
            "--tools",
            "core"
        ]
    );
    assert_eq!(unix.env["AGENT_BROWSER_STREAM_PORT"], "{stream_port}");
    assert_eq!(windows.env["AGENT_BROWSER_STREAM_PORT"], "{stream_port}");
    assert_eq!(unix.env["AGENT_BROWSER_SOCKET_DIR"], "{socket_dir}");
    assert!(!windows.env.contains_key("AGENT_BROWSER_SOCKET_DIR"));
    assert_eq!(unix.env.len(), 6);
    assert_eq!(windows.env.len(), 5);
}
