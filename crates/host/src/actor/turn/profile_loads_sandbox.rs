//! A Claude session's own sandbox (`agent::loads::sandbox`), gathered on the actor: the
//! session's folders, the data folder's spellings and the other matters' folders the launch
//! fenced (`agent::matter_fence`). Split from `profile_loads.rs` for its length.

use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, ThreadId};

use super::super::super::Actor;
use super::super::launch_checks::RunFolders;
use crate::agent::loads::sandbox::{ClaudeSandbox, Off, SandboxOs, SandboxPaths};

/// A Claude session's sandbox ([`claude_sandbox`]); none for another agent. Where one was meant
/// to be and is not, holds `auto` back for `thread`.
pub(super) fn session_sandbox(
    actor: &mut Actor,
    thread: ThreadId,
    kind: Option<AgentKind>,
    agent_id: &clauseo_protocol::AgentId,
    run: &RunFolders,
    matter: &Path,
) -> Option<ClaudeSandbox> {
    let sandbox =
        (kind == Some(AgentKind::Claude)).then(|| claude_sandbox(actor, agent_id, run, matter));
    if let Some(Err(off)) = sandbox
        && off.holds_auto_back()
    {
        // No sandbox where one was meant to be: no command may run on the classifier alone.
        super::super::session::hold_auto_back(actor, thread);
    }
    sandbox.and_then(Result::ok)
}

/// Claude Code's own sandbox for a Claude session of `agent_id` in `run`, whose matter folder
/// is `matter` (`agent::loads::sandbox`), or why there is none (on Windows, among others).
/// Path joins only.
fn claude_sandbox(
    actor: &Actor,
    agent_id: &clauseo_protocol::AgentId,
    run: &RunFolders,
    matter: &Path,
) -> Result<ClaudeSandbox, Off> {
    let tmp = crate::agent::temp::temp_dir(&run.root);
    let skills: Vec<PathBuf> = run
        .data_deny
        .spellings
        .iter()
        .map(|data| {
            crate::agent::data_fence::BUILT_IN_SKILLS
                .iter()
                .fold(data.clone(), |path, name| path.join(name))
        })
        .collect();
    let claude_home = actor.effective.locations().agent_home(agent_id.as_str());
    // Every other matter's folder the launch knew (forgotten ones too), but those the lawyer
    // let this conversation read (`agent::matter_fence`); one registered later is the residual.
    let fence = &run.data_deny.matters;
    let denies = sandbox_denies(run);
    let (inside, other_matters) = (denies.inside, denies.other_matters);
    let paths = SandboxPaths {
        matter,
        work: &run.work,
        tmp: &tmp,
        claude_tmp: run.claude_tmp.as_deref(),
        downloads: &run.downloads,
        skill_packs: &run.skill_packs,
        notes: &run.notes,
        skills: &skills,
        path: &run.path.dirs,
        env_file: run.path.env_file.as_deref(),
        ca_file: run.path.ca_file.as_deref(),
        claude_home: &claude_home,
        data: &run.data_deny.spellings,
        other_matters: &other_matters,
        matter_shut: fence.own_shut,
        matter_inside: &inside,
    };
    ClaudeSandbox::for_os(SandboxOs::CURRENT, &paths)
}

/// The other matters' folders Claude's sandbox denies, each as `deny_form` writes it, and
/// whether one had to be widened to a folder above it (the session's commands are limited).
pub(super) struct SandboxDenies {
    /// The other matters' folders inside the matter: denied writes.
    pub(super) inside: Vec<PathBuf>,
    /// Every fenced other matter's folder: denied reads.
    pub(super) other_matters: Vec<PathBuf>,
    /// Whether a deny was widened to a folder above what it names.
    pub(super) limited: bool,
}

/// [`SandboxDenies`] of `run`'s fence: each folder as registered, and where it leads
/// (`agent::loads::sandbox::deny_form`). Paths only.
pub(super) fn sandbox_denies(run: &RunFolders) -> SandboxDenies {
    let fence = &run.data_deny.matters;
    let stored: Vec<&Path> = fence
        .others
        .iter()
        .map(|other| other.folder.as_path())
        .collect();
    let mut limited = false;
    let mut form = |path: &Path| {
        let (path, widened) =
            crate::agent::loads::sandbox::deny_form(path, &stored, SandboxOs::CURRENT);
        limited |= widened;
        path
    };
    let inside = fence
        .inside_own
        .iter()
        .map(|(path, _)| form(path))
        .collect();
    let other_matters = fence.fenced_folders().map(&mut form).collect();
    SandboxDenies {
        inside,
        other_matters,
        limited,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clauseo_protocol::{MatterId, ThreadId};

    use super::{RunFolders, sandbox_denies};
    use crate::agent::loads::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
    use crate::agent::matter_fence::{MatterFence, Other};
    use crate::config::SessionKey;
    use crate::pipeline::tests::support::id;

    /// The sandbox's write list never opens an invited matter, in any spelling: a sibling's
    /// folders lie under no writable folder, and one inside the session's matter (writable) is
    /// denied writes in each spelling. An invite is read-only on macOS and Linux too.
    #[test]
    #[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
    fn the_sandbox_s_write_list_never_holds_an_invited_matter() {
        let data = PathBuf::from("/Data/Clauseo");
        let key = SessionKey {
            matter: id::<MatterId>(7),
            thread: id::<ThreadId>(8),
            launch: 1,
        };
        let run = RunFolders::new(&data, key);
        let matter = PathBuf::from("/Clients/A");
        let other = |n: u64, folder: &str, alias: &str, invited: bool| {
            let mut other = Other::new(id(n), format!("M{n}"), PathBuf::from(folder), invited);
            other.spellings = vec![PathBuf::from(folder), PathBuf::from(alias)];
            other
        };
        let sibling = other(1, "/Clients/B", "/Volumes/Share/B", true);
        let sibling_spellings = sibling.spellings.clone();
        let nested = other(2, "/Clients/A/Shared", "/Volumes/Share/A/Shared", true);
        let fenced = other(3, "/Clients/C", "/Volumes/Share/C", false);
        let invited: Vec<PathBuf> = [&sibling, &nested]
            .iter()
            .flat_map(|other| other.spellings.clone())
            .collect();
        let mut fence = MatterFence::asked(vec![sibling, nested.clone(), fenced]);
        fence.inside_own = nested.spellings.iter().map(|p| (p.clone(), true)).collect();
        let mut run = run;
        run.data_deny.matters = fence;
        let denies = sandbox_denies(&run);
        let tmp = crate::agent::temp::temp_dir(&run.root);
        let short = PathBuf::from("/tmp/clsb-abcd1234");
        let home = data.join("agents/claude-acp/home");
        let paths = SandboxPaths {
            matter: &matter,
            work: &run.work,
            tmp: &tmp,
            claude_tmp: Some(&short),
            downloads: &run.downloads,
            skill_packs: &run.skill_packs,
            notes: &run.notes,
            skills: &[],
            path: &[],
            env_file: None,
            ca_file: None,
            claude_home: &home,
            data: &run.data_deny.spellings,
            other_matters: &denies.other_matters,
            matter_shut: run.data_deny.matters.own_shut,
            matter_inside: &denies.inside,
        };
        for os in [SandboxOs::Mac, SandboxOs::Linux] {
            let sandbox = ClaudeSandbox::for_os(os, &paths).expect("the sandbox stays on");
            for spelling in &invited {
                let spelling = spelling.to_str().unwrap();
                let denied = sandbox.deny_write.iter().any(|path| path == spelling);
                let opened = sandbox.allow_write.iter().any(|open| {
                    let open = open.trim_end_matches("/**");
                    spelling == open || spelling.starts_with(&format!("{open}/"))
                });
                assert!(!opened || denied, "{os:?}: {spelling} is writable");
            }
            // Nor re-opened for reading: a sibling's folders lie under no `allowRead` entry, so
            // Claude's shell stays out where a `denyRead` (the home) holds them; one inside the
            // session's matter is read with the matter, which is re-opened whole.
            for spelling in sibling_spellings.iter().map(|p| p.to_str().unwrap()) {
                let read = sandbox.allow_read.iter().any(|open| {
                    let open = open.trim_end_matches("/**");
                    spelling == open || spelling.starts_with(&format!("{open}/"))
                });
                assert!(!read, "{os:?}: {spelling} is re-opened for reading");
            }
            assert!(
                sandbox
                    .deny_write
                    .iter()
                    .any(|path| path == "/Clients/A/Shared"),
                "{os:?}"
            );
        }
    }
}
