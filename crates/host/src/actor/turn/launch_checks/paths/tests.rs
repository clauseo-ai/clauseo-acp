//! Where a bare `stetdoc` resolves: stetdoc's folder is first on a session's `PATH`, so the
//! stetdoc allow rules (`clauseo_profile::claude::stetdoc_allowed`), which name the bare
//! command only, run Clauseo's stetdoc. A `stetdoc` in the matter folder is never first: the
//! working folder is not on `PATH` (and Claude Code 2.1.293 sets
//! `NoDefaultCurrentDirectoryInExePath=1` for its PowerShell), and
//! `./stetdoc` or `.\stetdoc` matches no stetdoc rule.

use clauseo_protocol::{AgentKind, MatterId, ThreadId};

use super::PathPrep;
use crate::config::SessionKey;
use crate::pipeline::tests::support::{TempDir, id};

/// Stetdoc's folder comes first, before the shell browser's tools and the profile hook's
/// folders, for both agents.
#[test]
fn stetdocs_folder_is_first_on_the_session_path() {
    let dir = TempDir::new("stetdoc-first");
    let key = SessionKey {
        matter: id::<MatterId>(7),
        thread: id::<ThreadId>(8),
        launch: 1,
    };
    let mut run = super::RunFolders::new(dir.path(), key);
    crate::write::create_dir_all(&run.root).unwrap();
    run.browser_shell = true;
    run.chrome = Some(dir.path().join("chrome"));
    let stetdoc = dir.path().join("app").join("stetdoc");
    let tools = dir.path().join("tools");
    let hook = dir.path().join("hook");
    for kind in [AgentKind::Claude, AgentKind::Codex] {
        let prep = PathPrep {
            prepend: Some(stetdoc.clone()),
            browser_tool: Some(tools.clone()),
            tls_roots: None,
            hook: (vec![hook.clone()], true),
            kind: Some(kind),
        };
        let path = prep.resolve(&run);
        assert_eq!(
            path.dirs,
            [stetdoc.clone(), tools.clone(), hook.clone()],
            "{kind:?}"
        );
    }
}
