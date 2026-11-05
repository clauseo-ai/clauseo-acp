//! What the plan may not write: the profile, the sandbox records, and the data folder's allow
//! list ([`check_target`]).

use std::path::{Component, Path};

use clauseo_protocol::MatterId;

use super::{Grant, Rights, in_folder};
use crate::windows::admit::{Admitted, FolderRefusal, PathKind};
use crate::windows::matters::SANDBOX_DIR;

/// Why `grant` may not be written, if it may not. `<data_dir>` is an allow list:
///
/// - any path with a name that ends in `.` or a space, or holds `:` (an NTFS stream, such as
///   `home::$INDEX_ALLOCATION`), is refused first ([`FolderRefusal::OddSpelling`]): Windows reads
///   each as another spelling of a name;
/// - on the profile folder or above it ([`FolderRefusal::HoldsTheProfile`]);
/// - on, over or in `<data_dir>\sandbox` ([`FolderRefusal::OverlapsSandboxRecords`]);
/// - a deny, or a path entirely outside `<data_dir>`, is fine;
/// - a path that is `<data_dir>` or holds it is refused, and inside it
///   ([`FolderRefusal::OverlapsDataFolder`] for anything not listed here):
///   - writable: the state folder exactly one level below `<data_dir>\agent-state\<matter>`
///     (not `notes`), the notes folder exactly `<data_dir>\agent-state\<matter>\notes`, the
///     credential link inside that state folder, and the Claude store exactly
///     `<data_dir>\agents\claude-acp\home` and its allowed names, `matter` being this
///     launch's matter id, never a string of the policy's;
///   - read and run: inside one agent's folder under `<data_dir>\agents`, never in its `home`
///     unless it is that Claude store.
pub(super) fn check_target(
    admitted: &Admitted<'_>,
    grant: &Grant,
    matter: MatterId,
) -> Option<FolderRefusal> {
    let policy = admitted.policy();
    let path = &grant.path;
    if odd_spelling(path) {
        return Some(FolderRefusal::OddSpelling);
    }
    if policy.home().is_some_and(|home| in_folder(home, path)) {
        return Some(FolderRefusal::HoldsTheProfile);
    }
    let data = admitted.data_dir();
    let sandbox = data.join(SANDBOX_DIR);
    if in_folder(path, &sandbox) || in_folder(&sandbox, path) {
        return Some(FolderRefusal::OverlapsSandboxRecords);
    }
    if grant.rights.is_deny() || !(in_folder(path, data) || in_folder(data, path)) {
        return None;
    }
    let allowed = in_folder(path, data) && !same_path(path, data) && {
        let mine = data.join(AGENT_STATE_DIR).join(matter.to_string());
        let store = data
            .join(AGENTS_DIR)
            .join(CLAUDE_AGENT)
            .join(AGENT_HOME_DIR);
        let state_ok = |state: &Path| {
            state
                .parent()
                .is_some_and(|parent| same_path(parent, &mine))
                && !state
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(NOTES_DIR))
        };
        match grant.rights {
            Rights::ReadAndExecute => same_path(path, &store) || in_agent_install(path, data),
            Rights::Modify | Rights::CreateFiles | Rights::LowLabel => match grant.kind {
                PathKind::State => state_ok(path),
                PathKind::Notes => same_path(path, &mine.join(NOTES_DIR)),
                PathKind::CredentialLink => {
                    state_ok(policy.state()) && in_folder(path, policy.state())
                }
                PathKind::ClaudeStore => same_path(path, &store),
                PathKind::ClaudeStoreFile => path.parent().is_some_and(|p| same_path(p, &store)),
                PathKind::Matter
                | PathKind::Runtime
                | PathKind::Read
                | PathKind::AllowedFolder
                | PathKind::ReadOnlyInMatter
                | PathKind::ReadOnlyInState
                | PathKind::OtherMatter
                | PathKind::Sibling
                | PathKind::ParentFiles => false,
            },
            Rights::DenyWrite | Rights::DenyReadWrite | Rights::DenyRead => true,
        }
    };
    (!allowed).then_some(FolderRefusal::OverlapsDataFolder)
}

/// Whether `path` is inside one agent's folder under `<data_dir>\agents` (not the folder
/// itself, which holds the home) and outside its `home`.
fn in_agent_install(path: &Path, data: &Path) -> bool {
    let agents = data.join(AGENTS_DIR);
    if !in_folder(path, &agents) {
        return false;
    }
    let below: Vec<Component<'_>> = path
        .components()
        .skip(agents.components().count())
        .collect();
    let in_home = below
        .get(1)
        .is_some_and(|part| part.as_os_str().eq_ignore_ascii_case(AGENT_HOME_DIR));
    below.len() >= 2 && !in_home
}

/// Whether `a` and `b` are the same path, by [`in_folder`]'s comparison.
fn same_path(a: &Path, b: &Path) -> bool {
    a.components().count() == b.components().count() && in_folder(a, b)
}

/// Whether a name in `path` is a spelling Windows reads as another name: one ending in `.` or
/// a space, or one holding `:` (a stream; the drive's colon is in the prefix, not a name).
/// Short 8.3 names need no rule here: every policy path, and the data folder, is canonical
/// through the final-path API (`dunce::canonicalize`, `GetFinalPathNameByHandleW` on Windows),
/// which gives long names, and a `~` is an ordinary character (`Draft~2`).
pub(crate) fn odd_spelling(path: &Path) -> bool {
    path.components().any(|part| {
        let Component::Normal(name) = part else {
            return false;
        };
        let name = name.to_string_lossy();
        name.ends_with('.') || name.ends_with(' ') || name.contains(':')
    })
}

/// `<data_dir>\agents`: each agent's install folder, its runtime and its home.
pub(crate) const AGENTS_DIR: &str = "agents";

/// `<data_dir>\agent-state`: each matter's state and notes folders, by matter id.
const AGENT_STATE_DIR: &str = "agent-state";

/// A matter's notes folder in `<data_dir>\agent-state\<matter>`.
const NOTES_DIR: &str = "notes";

/// The one agent whose home may be the Claude store: Clauseo's own Claude sign-in.
const CLAUDE_AGENT: &str = "claude-acp";

/// An agent's home in its install folder, `<data_dir>\agents\<agent>\home`: its sign-in.
const AGENT_HOME_DIR: &str = "home";
