//! The lawyer's servers rewritten to `clauseo-mcp launch`, their files written, and the sweep's
//! programs: pure parts against values, the write against a temp folder.

use std::path::PathBuf;

use clauseo_acp::McpServer;
use serde_json::{Value, json};

use super::{MODE, Staged, file_name, rewrite, write};
use crate::pipeline::tests::support::TempDir;

const MARKER: &str = "MARKER-SECRET-123";

fn abs(path: &str) -> PathBuf {
    PathBuf::from(crate::testing::abs(path))
}

fn launcher() -> PathBuf {
    abs("/opt/clauseo/clauseo-mcp")
}

fn program(name: &str) -> PathBuf {
    abs(&format!("/opt/servers/{name}"))
}

fn local(name: &str) -> McpServer {
    McpServer::Stdio {
        name: name.to_owned(),
        command: program(name),
        args: vec!["--token".to_owned(), MARKER.to_owned()],
        env: vec![
            ("TOKEN".to_owned(), MARKER.to_owned()),
            ("PATH".to_owned(), "bin-folder".to_owned()),
        ],
    }
}

fn remote() -> McpServer {
    McpServer::Http {
        name: "docs".to_owned(),
        url: "https://mcp.example/x".to_owned(),
        headers: vec![("Authorization".to_owned(), "Bearer t".to_owned())],
    }
}

/// Every local entry becomes the launcher with `launch` and its file, and holds no value of the
/// server; the remote one is left as it is (the bridge's follow-up); the order stays.
#[test]
fn the_entries_hold_no_argument_or_variable_of_the_server() {
    let run = abs("/opt/data/run/s1");
    let basics = vec![("SystemRoot".to_owned(), "C:\\Windows".to_owned())];
    let (entries, files) = rewrite(
        vec![local("alpha"), remote(), local("beta")],
        &launcher(),
        &run,
        &basics,
    );
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[1], remote());
    for (entry, (index, name)) in [&entries[0], &entries[2]]
        .into_iter()
        .zip([(0, "alpha"), (2, "beta")])
    {
        let McpServer::Stdio {
            name: sent,
            command,
            args,
            env,
        } = entry
        else {
            panic!("{entry:?}");
        };
        let path = run.join("mcp").join(format!("{index}-{name}.json"));
        assert_eq!(sent, name);
        assert_eq!(command, &launcher());
        assert_eq!(args, &[MODE.to_owned(), path.to_str().unwrap().to_owned()]);
        assert_eq!(env, &basics);
        let text = format!("{entry:?}{}", serde_json::to_string(&json!(args)).unwrap());
        assert!(!text.contains(MARKER), "{text}");
    }
    assert_eq!(files.len(), 2);
}

/// A file holds the real program, arguments and variables, in `clauseo-mcp launch`'s shape.
#[test]
fn the_file_holds_the_real_values() {
    let run = abs("/opt/data/run/s1");
    let (_, files) = rewrite(vec![local("alpha")], &launcher(), &run, &[]);
    let body: Value = serde_json::from_slice(&files[0].bytes).unwrap();
    assert_eq!(
        body,
        json!({"v": 1, "command": program("alpha").to_str().unwrap(),
            "args": ["--token", MARKER], "env": [["TOKEN", MARKER], ["PATH", "bin-folder"]]})
    );
    assert_eq!(files[0].program, program("alpha"));
}

/// The place in the list comes first, so names that differ in case never share a file; a name
/// that is not name-shaped never reaches the path.
#[test]
fn file_names_are_unique_and_never_a_path() {
    assert_eq!(file_name(0, "Github"), "0-Github.json");
    assert_eq!(file_name(1, "github"), "1-github.json");
    assert_eq!(file_name(2, "../x"), "2.json");
    assert_eq!(file_name(3, ""), "3.json");
    assert_eq!(file_name(4, &"a".repeat(65)), "4.json");
}

/// The files written are private (0600, in a 0700 folder on Unix), hold the values, and a
/// second write (the next session call of the run) replaces them in place.
#[test]
fn the_write_makes_private_files_and_a_second_write_replaces_them() {
    let dir = TempDir::new("mcp-launch-write");
    let run = dir.path().join("run").join("s1");
    crate::write::create_dir_all(&run).unwrap();
    let (_, files) = rewrite(vec![local("alpha")], &launcher(), &run, &[]);
    write(&run, &files).unwrap();
    write(&run, &files).unwrap();
    let path = run.join("mcp").join("0-alpha.json");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(MARKER));
    let names: Vec<_> = std::fs::read_dir(run.join("mcp"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["0-alpha.json"], "no temp file is left");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&run.join("mcp")), 0o700);
    }
}

/// Only the files of servers the call sends are kept, and the sweep spares their real programs
/// beside the launcher: on Unix the mode becomes the real program.
#[test]
fn the_sweep_keeps_the_real_programs_of_the_servers_sent() {
    let run = abs("/opt/data/run/s1");
    let (entries, files) = rewrite(vec![local("alpha"), local("beta")], &launcher(), &run, &[]);
    let mut staged = Staged { root: run, files };
    // `beta` is not sent (the firm's managed-mcp.json, say): its file is not written.
    let sent = vec![entries[0].clone()];
    let keep = staged.sent(&sent, vec![launcher()]);
    assert_eq!(keep, [launcher(), program("alpha")]);
    assert_eq!(staged.files.len(), 1);
    assert_eq!(staged.files[0].program, program("alpha"));
}
