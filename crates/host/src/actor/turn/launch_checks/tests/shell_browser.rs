//! The shell browser's run folders, config and profile, and its environment under the session's
//! Codex fence. Split from `tests.rs` for its size.

use std::path::{Path, PathBuf};

use clauseo_protocol::{MatterId, ThreadId};

use super::{RunFolders, RunSweep, folders, names, scratch};
use crate::config::SessionKey;
use crate::pipeline::tests::support::id;
use crate::write;

/// The shell browser's environment for a session, built from its run folders as the launch
/// builds them, against that session's Codex fence as the launch lists it: `AGENT_BROWSER_CONFIG`,
/// `_DOWNLOAD_PATH` and `_PROFILE` and this matter's brief all writable, to an ordinary command
/// and to an escalated one (agent-browser's always is), the `agent-browser` copy in
/// `<data>/tools` readable and not writable to an ordinary command; another matter's session's
/// three and its brief denied to both, and so is `<data>/browser`.
#[test]
fn the_shell_browser_env_is_reachable_under_its_own_codex_fence_only() {
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    use crate::agent::codex_fence::tests::{escalated_access, session_access};
    use crate::agent::codex_fence::{Session, keys, list};
    use crate::agent::loads::browser::{ShellInputs, shell_env};

    let root = scratch("abfence-combined");
    let locations = clauseo_settings::Locations::under_root(&root);
    let data = locations.data_dir().to_path_buf();
    let sweep = RunSweep::default();
    let run = folders(&data, 1);
    let other = RunFolders::new(
        &data,
        SessionKey {
            matter: id::<MatterId>(9),
            thread: id::<ThreadId>(10),
            launch: 1,
        },
    );
    for folders in [&run, &other] {
        folders.make(&data, &sweep).unwrap();
        write::create_dir_all(&folders.browser_profile).unwrap();
        write::create_dir_all(&folders.downloads).unwrap();
    }
    let listed = list(&data, &run.root, &run.notes);
    let session = Session {
        run_root: &run.root,
        notes: &run.notes,
        codex_home: &locations.agent_home("codex-acp"),
        listed: &listed,
        socket_dir: None,
        other_matters: &[],
    };
    let fence = keys(&locations, session, false);
    let own = [run.root.clone(), run.notes.clone()];
    let env_of = |run: &RunFolders| -> BTreeMap<OsString, OsString> {
        let id = run.key.id();
        shell_env(&ShellInputs {
            session: &id,
            config: &run.browser_config,
            downloads: &run.downloads,
            profile: &run.browser_profile,
            executable: Path::new("/opt/chrome"),
        })
    };
    let names = [
        "AGENT_BROWSER_CONFIG",
        "AGENT_BROWSER_DOWNLOAD_PATH",
        "AGENT_BROWSER_PROFILE",
    ];
    let paths = |run: &RunFolders| -> Vec<PathBuf> {
        let env = env_of(run);
        let mut paths: Vec<PathBuf> = names
            .iter()
            .map(|name| PathBuf::from(&env[&OsString::from(name)]))
            .collect();
        paths.push(run.browser_profile.join("Default").join("Cookies"));
        paths.push(run.notes.join("matter-brief.md"));
        paths
    };
    for path in paths(&run) {
        assert_eq!(
            session_access(&fence, &own, &path),
            "write",
            "{}",
            path.display()
        );
        assert_eq!(
            escalated_access(&fence, &path),
            "write",
            "{}",
            path.display()
        );
    }
    for path in &paths(&run)[..3] {
        assert!(path.starts_with(&run.agent_browser), "{}", path.display());
    }
    let tools = data.join(clauseo_profile::paths::TOOLS_DIR);
    let copy = tools.join(clauseo_profile::paths::AGENT_BROWSER_COMMAND);
    assert_eq!(session_access(&fence, &own, &copy), "read");
    let app_wide = data
        .join(crate::browser::CONFIG_DIR)
        .join(crate::browser::CONFIG_FILE);
    for path in paths(&other).into_iter().chain([app_wide]) {
        assert_eq!(
            session_access(&fence, &own, &path),
            "deny",
            "{}",
            path.display()
        );
        assert_eq!(
            escalated_access(&fence, &path),
            "deny",
            "{}",
            path.display()
        );
    }
    run.remove();
    other.remove();
    write::remove_dir_all(&root).unwrap();
}

/// The shell tool's folders and config are made in the run folder's own `agent-browser`
/// folder, the config `{}` and private, a planted one replaced at the next launch.
#[test]
fn the_shell_browser_folders_and_config_are_made_in_the_own_agent_browser_folder() {
    let data = scratch("agent-browser");
    let mut run = folders(&data, 1);
    run.make(&data, &RunSweep::default()).unwrap();
    run.chrome = Some(PathBuf::from("/opt/chrome"));
    super::browser::shell_folders(&mut run, &data);
    assert!(run.chrome.is_some());
    assert_eq!(run.agent_browser, run.root.join("agent-browser"));
    assert!(run.downloads.is_dir() && run.browser_profile.is_dir());
    assert_eq!(std::fs::read(&run.browser_config).unwrap(), b"{}\n");
    assert_eq!(
        names(&run.agent_browser),
        ["config.json", "downloads", "profile"]
    );
    assert!(!data.join(crate::browser::CONFIG_DIR).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&run.browser_config), 0o600);
        assert_eq!(mode(&run.agent_browser), 0o700);
    }
    write::write_file(&run.browser_config, br#"{"provider":"browserbase"}"#).unwrap();
    super::browser::shell_folders(&mut run, &data);
    assert_eq!(std::fs::read(&run.browser_config).unwrap(), b"{}\n");
    run.remove();
    write::remove_dir_all(&data).unwrap();
}

/// The shell tool's profile saves a PDF link rather than opening it: its `Default/Preferences`
/// holds `always_open_pdf_externally`, and a planted one is replaced at the next launch.
#[test]
fn the_shell_profile_saves_a_pdf_link_instead_of_opening_it() {
    let data = scratch("profile-preferences");
    let mut run = folders(&data, 1);
    run.make(&data, &RunSweep::default()).unwrap();
    run.chrome = Some(PathBuf::from("/opt/chrome"));
    let file = run.browser_profile.join("Default").join("Preferences");
    let on = r#"{"plugins":{"always_open_pdf_externally":true}}"#;
    super::browser::shell_folders(&mut run, &data);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), on);
    write::write_file(&file, b"{}").unwrap();
    super::browser::shell_folders(&mut run, &data);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), on);
    run.remove();
    write::remove_dir_all(&data).unwrap();
}
