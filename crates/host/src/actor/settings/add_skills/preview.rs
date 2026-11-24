//! `SearchSkills` and `PreviewSkill`: each answered from a task of its own, the network or
//! the disk off the actor, so the host thread never waits on them.
//!
//! A preview previews (downloads or reads) only now, sweeps the previews past their 30 minutes,
//! and answers the skills that can be added: one too big or breaking a pack rule is left out,
//! and when that leaves none, the answer is `skill.too_big` (any too big) or `skill.invalid`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_profile::packs::added;
use clauseo_profile::packs::source::{
    ADDING_DIR, FromKind, Preview, SourceError, close_preview, parse_link, parse_skills_sh,
    preview_local, sweep,
};
use clauseo_protocol::{
    AddonFrom, AddonFromKind, CatalogueItem, PreviewedSkill, QueryResult, SkillFrom, SkillPreview,
    SkillSearch,
};
use clauseo_sandbox::Upstream;
use futures::task::LocalSpawnExt as _;

use super::super::super::Actor;
use super::super::super::input::Reply;
use super::{packs_root, source_error};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::error::Error;
use crate::skill_source::download::{DOWNLOAD_TIMEOUT, Net, preview_remote};
use crate::skill_source::search::search as catalogue;

/// The job a preview or a search runs as: a download's own bound, and a little more.
const PREVIEW_JOB: Job = Job {
    what: "skill preview",
    matter: None,
    wait: DOWNLOAD_TIMEOUT.saturating_add(Duration::from_secs(30)),
};

/// skills.sh's pages.
const PAGE: &str = "https://skills.sh/";

/// How a task off the actor reaches the network: the fake server's origin in tests, else the
/// `network.upstream_proxy` setting and the system's proxy.
#[derive(Clone)]
pub(super) struct Route {
    test_server: Option<String>,
    upstream_proxy: Option<String>,
}

impl Route {
    pub(super) fn of(actor: &Actor) -> Self {
        Self {
            test_server: actor.wiring.skill_server.clone(),
            upstream_proxy: actor.effective.settings().network.upstream_proxy.clone(),
        }
    }

    pub(super) fn net(&self) -> Result<Net, SourceError> {
        match &self.test_server {
            Some(origin) => Ok(Net::plain(origin)),
            None => Net::new(self.upstream_proxy.as_deref(), &Upstream::System),
        }
    }
}

/// `SearchSkills`.
pub(in crate::actor) fn search(actor: &Actor, q: String, reply: Reply<QueryResult>) {
    let route = Route::of(actor);
    answer(actor, reply, move || {
        let items = catalogue(&route.net().map_err(source_error)?, &q).map_err(source_error)?;
        let items = items
            .into_iter()
            .map(|item| CatalogueItem {
                page: format!("{PAGE}{}", item.id),
                id: item.id,
                name: item.name,
                source: item.source,
                installs: u32::try_from(item.installs).unwrap_or(u32::MAX),
            })
            .collect();
        Ok(QueryResult::SkillSearch(SkillSearch { items }))
    });
}

/// `PreviewSkill`.
pub(in crate::actor) fn preview(actor: &Actor, from: SkillFrom, reply: Reply<QueryResult>) {
    let route = Route::of(actor);
    let adding = actor.effective.data_dir().join(ADDING_DIR);
    let root = packs_root(actor);
    let now = actor.clock.now().wall.as_millis();
    answer(actor, reply, move || {
        sweep(&adding, Some(now));
        let staged = stage(&route, &from, &adding, now).map_err(source_error)?;
        shown(staged, &root, &adding).map(QueryResult::SkillPreview)
    });
}

/// Runs `work` off the actor and sends its answer to `reply`.
fn answer(
    actor: &Actor,
    reply: Reply<QueryResult>,
    work: impl FnOnce() -> Result<QueryResult, Error> + Send + 'static,
) {
    let clock = Arc::clone(&actor.clock);
    let spawned = actor.spawner.spawn_local(async move {
        let _gone = reply.send(off_actor(clock, PREVIEW_JOB, work).await);
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "the host pool refused a skill preview or search");
    }
}

/// Downloads or reads `from` and stages it.
fn stage(route: &Route, from: &SkillFrom, adding: &Path, now: i64) -> Result<Preview, SourceError> {
    let remote = match from {
        SkillFrom::Path(path) => return preview_local(adding, &PathBuf::from(path), now),
        SkillFrom::SkillsSh(name) => parse_skills_sh(name)?,
        SkillFrom::Url(link) => parse_link(link)?,
    };
    preview_remote(&route.net()?, &remote, adding, now)
}

/// The answer for `preview`: the skills that can be added, each with whether a skill added
/// here holds its id; none left, the preview is closed and refused.
fn shown(preview: Preview, root: &Path, adding: &Path) -> Result<SkillPreview, Error> {
    let too_big = preview
        .skills
        .iter()
        .any(|skill| skill.problem == Some(clauseo_profile::packs::Refusal::TooBig));
    let skills: Vec<PreviewedSkill> = preview
        .skills
        .iter()
        .filter(|skill| skill.problem.is_none())
        .map(|skill| PreviewedSkill {
            name: skill.name.clone(),
            description: skill.description.chars().take(200).collect(),
            has_scripts: skill.has_scripts,
            already_added: skill
                .slug
                .as_ref()
                .is_some_and(|slug| added::is_added_here(&root.join(slug))),
        })
        .collect();
    if skills.is_empty() {
        close_preview(adding, &preview.token);
        let error = if too_big {
            SourceError::TooBig
        } else {
            SourceError::Unsafe
        };
        return Err(source_error(error));
    }
    Ok(SkillPreview {
        token: preview.token,
        from: AddonFrom {
            kind: from_kind(preview.origin.kind),
            text: preview.origin.text,
        },
        skills,
    })
}

/// The wire's kind of place for `kind`.
pub(in crate::actor) fn from_kind(kind: FromKind) -> AddonFromKind {
    match kind {
        FromKind::SkillsSh => AddonFromKind::SkillsSh,
        FromKind::GitHub => AddonFromKind::Github,
        FromKind::Url => AddonFromKind::Url,
        FromKind::Zip => AddonFromKind::Zip,
        FromKind::Folder => AddonFromKind::Folder,
    }
}
