//! A Claude session's sandbox for its session call (`agent::loads::sandbox`), gathered on the
//! actor: on macOS and Linux the sandbox or the launch's refusal, never neither. Split from
//! `profile_loads.rs` for its size.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentId, AgentKind, Notice, NoticeKind, NoticeLevel, ThreadId};

use super::super::super::Actor;
use super::super::launch_checks::RunFolders;
use crate::agent::loads::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
use crate::error::Error;
use crate::log_file::Cause;

/// A Claude session's sandbox ([`claude_sandbox`]); none for another agent, nor on Windows.
/// Where one was meant to be and can't be built, the launch is refused
/// (`ClaudeSandbox::for_launch`), with one warn line naming the matter.
pub(super) fn session_sandbox(
    actor: &Actor,
    thread: ThreadId,
    kind: Option<AgentKind>,
    agent_id: &AgentId,
    run: &RunFolders,
    matter: &Path,
) -> Result<Option<ClaudeSandbox>, Error> {
    if kind != Some(AgentKind::Claude) {
        return Ok(None);
    }
    claude_sandbox(actor, agent_id, run, matter).inspect_err(|error| {
        let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
        tracing::warn!(
            %thread,
            matter = ?matter,
            error = %Cause(error),
            "a Claude launch has no sandbox on macOS or Linux: the launch is refused"
        );
    })
}

/// Claude Code's own sandbox for a Claude session of `agent_id` in `run`, whose matter folder
/// is `matter` (`agent::loads::sandbox`): none on Windows, else it or the launch's refusal.
/// Path joins only.
fn claude_sandbox(
    actor: &Actor,
    agent_id: &AgentId,
    run: &RunFolders,
    matter: &Path,
) -> Result<Option<ClaudeSandbox>, Error> {
    let tmp = crate::agent::temp::temp_dir(&run.root);
    let skills: Vec<PathBuf> = run
        .data_deny
        .spellings
        .iter()
        .map(|data| {
            // Under a linked data folder, the real spelling alone (`folders::respell_run`).
            crate::agent::data_fence::BUILT_IN_SKILLS
                .iter()
                .fold(run.real(data), |path, name| path.join(name))
        })
        .collect();
    // Under a linked data folder, its real spelling (`launch_checks::folders::respell_run`).
    let claude_home = run.real(&actor.effective.locations().agent_home(agent_id.as_str()));
    let paths = SandboxPaths {
        matter,
        matter_walked: run.matter_walked.as_deref(),
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
        other_matters: &run.other_matters,
    };
    ClaudeSandbox::for_launch(SandboxOs::CURRENT, &paths)
}

/// The notice's fallback words, for a view that does not know its kind (UC-846's English with
/// "Claude": only Claude's sandbox follows other matters' links).
const LINK_TEXT: &str = "The folder for {matter} has been replaced by a shortcut to somewhere \
    else, so Clauseo can no longer keep Claude out of the place it leads to. Check that folder \
    before you open {matter} again.";

/// A thread's notices of other matters whose folders lead somewhere Claude's sandbox must leave
/// open (`ClaudeSandbox::links_left_out`): what its last launch left out, until its session
/// attaches, and the matters it was already told of, for the host's run.
#[derive(Debug, Default)]
pub(crate) struct LinkNotices {
    pending: Vec<String>,
    shown: HashSet<String>,
}

impl LinkNotices {
    /// Holds `matters`, the last launch's, until its session attaches.
    fn hold(&mut self, matters: Vec<String>) {
        self.pending = matters;
    }

    /// The notices for what is pending that the thread was not told of yet, one per matter: a
    /// warning of kind `OtherMatterLinkUnexpected` naming the matter, never a path.
    fn untold(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .filter(|matter| self.shown.insert(matter.clone()))
            .map(|matter| Notice {
                level: NoticeLevel::Warning,
                text: LINK_TEXT.replace("{matter}", &matter),
                kind: Some(NoticeKind::OtherMatterLinkUnexpected { matter }),
            })
            .collect()
    }
}

/// Holds the other matters `sandbox` left out (the launch of `thread`) for its session's notice.
pub(super) fn hold_link_notices(
    actor: &mut Actor,
    thread: ThreadId,
    sandbox: Option<&ClaudeSandbox>,
) {
    if let Some(state) = super::super::state(actor, thread) {
        let left_out = sandbox.map(|sandbox| sandbox.links_left_out.clone());
        state.links.hold(left_out.unwrap_or_default());
    }
}

/// `thread`'s notices of other matters whose folders lead somewhere unexpected, for its
/// session's attach ([`LinkNotices`]).
pub(crate) fn link_notices(actor: &mut Actor, thread: ThreadId) -> Vec<Notice> {
    super::super::state(actor, thread).map_or_else(Vec::new, |state| state.links.untold())
}

#[cfg(test)]
mod tests {
    use clauseo_protocol::{NoticeKind, NoticeLevel};

    use super::LinkNotices;

    /// UC-846: one warning per matter left out, naming it and never a path, told once for the
    /// host's run; a later launch with none left out tells nothing.
    #[test]
    fn a_matter_whose_folder_leads_somewhere_unexpected_is_told_once() {
        let mut notices = LinkNotices::default();
        notices.hold(vec!["Acme v Jones".to_owned()]);
        let told = notices.untold();
        assert_eq!(told.len(), 1);
        let notice = told.first().unwrap();
        assert_eq!(notice.level, NoticeLevel::Warning);
        assert_eq!(
            notice.kind,
            Some(NoticeKind::OtherMatterLinkUnexpected {
                matter: "Acme v Jones".to_owned()
            })
        );
        assert!(
            notice
                .text
                .starts_with("The folder for Acme v Jones has been replaced")
        );
        assert!(notice.text.contains("keep Claude out"));
        assert!(!notice.text.contains('/'));
        notices.hold(vec!["Acme v Jones".to_owned()]);
        assert!(notices.untold().is_empty());
        notices.hold(Vec::new());
        assert!(notices.untold().is_empty());
    }
}
