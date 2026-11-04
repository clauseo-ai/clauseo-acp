//! The host's usage-count call sites (decision record 0036, S21): each turns what the host just
//! did into a closed `clauseo_counts` event and bumps it into the counts `HostConfig::with_counts`
//! gave. Only enums and buckets leave here: no name, path, text or id.
//!
//! Counting never blocks the actor and never fails a command. `Counts::bump` takes one lock and
//! adds one; with no counts it is a no-op with a debug log, and a counts error never reaches a
//! command. Each event is counted once per operation that succeeded, at the point it became
//! durable, never per attempt: a refused or failed command counts nothing, and a repeated
//! command id answered from its receipt makes no commit, so it counts nothing either.
//!
//! - `app_opened` (`cold`): the host has started (`Actor::settle`).
//! - `matter_opened`: `CreateMatter`'s commit; `OpenMatter`'s commit, with the folder's papers
//!   listed on the matter's worker. `place` (`clauseo_matters::place`, which stats the folder)
//!   runs on the one `clauseo-place` thread, never on the actor, and the event counts there.
//! - `question_sent`: the prompt's commit, started now, queued, sent now over a running answer,
//!   or a steer.
//! - `answer_ended`: `turn::end::finish`, the one place every turn ends.
//! - `feature_used`: `undo` and `restore`, once their log is durable.
//! - `setting_changed`: after a `SetUiSettings` or `SetPrivacySettings` write, per value that
//!   changed.

use std::path::PathBuf;
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

use clauseo_counts::schema::{
    Agent, AnswerEnded, AnswerOutcome, AppOpened, Delivery, Event, Feature, FeatureUsed, Language,
    Launch, LiteMode, MatterKind, MatterOpened, Papers, Place, Protection, QuestionSent,
    ReducedMotion, SettingChanged, TextSize, Theme, Took,
};
use clauseo_counts::{Counts, Mode};
use clauseo_protocol::{
    AgentKind, CommandId, ErrorCode, ImportSummary, MatterId, QueryResult, StopReason, ThreadId,
};
use clauseo_settings as settings;
use futures::channel::oneshot;
use futures::task::LocalSpawnExt;

use super::Actor;

/// What to count once a command's commit is durable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OnCommit {
    /// `matter_opened` for a matter `CreateMatter` made in `folder`, once its place is known.
    MatterCreated {
        kind: MatterKind,
        papers: Papers,
        folder: PathBuf,
    },
    /// `matter_opened` for this matter, once its papers are listed and its place is known.
    MatterOpened(MatterId),
}

/// Counts `event` once, or nothing when the host has no counts.
pub(crate) fn bump(actor: &Actor, event: Event) {
    if let Some(counts) = &actor.counts {
        counts.bump(event);
    } else {
        tracing::debug!(
            ?event,
            "no usage counts in this host, so nothing is counted"
        );
    }
}

/// `app_opened`: the host has started. `after_update` waits for R23's update marker.
pub(super) fn app_opened(actor: &Actor) {
    bump(
        actor,
        Event::AppOpened(AppOpened {
            launch: Launch::Cold,
        }),
    );
}

/// Counts `what` when command `id`'s commit, just sent, is durable ([`committed`]). Nothing is
/// kept while counts are off or absent.
pub(super) fn on_commit(actor: &mut Actor, id: CommandId, what: OnCommit) {
    if counting(actor).is_some() {
        actor.count_on_commit.insert(id, what);
    }
}

/// Command `id`'s commit returned: what [`on_commit`] put by for it counts now when it is
/// durable, and is dropped when it failed.
pub(super) fn committed(actor: &mut Actor, id: CommandId, durable: bool) {
    let Some(what) = actor.count_on_commit.remove(&id) else {
        return;
    };
    if !durable {
        return;
    }
    match what {
        OnCommit::MatterCreated {
            kind,
            papers,
            folder,
        } => {
            if let Some(counts) = counting(actor) {
                bump_matter(counts, kind, papers, folder);
            }
        }
        OnCommit::MatterOpened(matter) => matter_opened(actor, matter),
    }
}

/// The counts, while they are on; none (with a debug log) while off or absent, so nothing is
/// listed or statted for them.
fn counting(actor: &Actor) -> Option<Counts> {
    let counts = actor
        .counts
        .clone()
        .filter(|counts| counts.mode() != Mode::Off);
    if counts.is_none() {
        tracing::debug!("usage counts are off or absent: nothing is counted");
    }
    counts
}

/// `matter_opened` for a matter `CreateMatter` made in `folder`: a folder the lawyer had
/// (imported, with its papers) or a new one (no papers). `protection` waits for R17's sandbox.
pub(super) fn matter_created(import: Option<&ImportSummary>, folder: PathBuf) -> OnCommit {
    let kind = match import {
        Some(_) => MatterKind::Folder,
        None => MatterKind::NewFolder,
    };
    let papers = Papers::from_count(import.map_or(0, |import| u64::from(import.files)));
    OnCommit::MatterCreated {
        kind,
        papers,
        folder,
    }
}

/// One `matter_opened` waiting for its folder's place.
struct PlaceJob {
    counts: Counts,
    kind: MatterKind,
    papers: Papers,
    folder: PathBuf,
}

/// The one `clauseo-place` thread, started at its first job and kept for the process: a stat a
/// network share holds up holds up only the counts queued behind it, never the actor, and
/// strands one thread at most.
static PLACES: OnceLock<Option<mpsc::Sender<PlaceJob>>> = OnceLock::new();

/// Counts `matter_opened` from the `clauseo-place` thread, once `clauseo_matters::place` has
/// said where `folder` lives: it stats the folder, so never on the actor. Only the place's kind
/// is counted, never the sync service's name.
fn bump_matter(counts: Counts, kind: MatterKind, papers: Papers, folder: PathBuf) {
    let places = PLACES.get_or_init(|| {
        let (send, jobs) = mpsc::channel::<PlaceJob>();
        let started = std::thread::Builder::new()
            .name("clauseo-place".to_owned())
            .spawn(move || {
                for job in jobs {
                    let place = match clauseo_matters::place(&job.folder) {
                        clauseo_matters::Place::Local => Place::Local,
                        clauseo_matters::Place::Network => Place::Network,
                        clauseo_matters::Place::Synced { .. } => Place::CloudSync,
                    };
                    job.counts.bump(Event::MatterOpened(MatterOpened {
                        kind: job.kind,
                        papers: job.papers,
                        place: Some(place),
                        protection: Protection::NotProtected,
                    }));
                }
            });
        match started {
            Ok(_) => Some(send),
            Err(error) => {
                tracing::debug!(%error, "no thread to read matters' places for their counts");
                None
            }
        }
    });
    let job = PlaceJob {
        counts,
        kind,
        papers,
        folder,
    };
    let sent = places.as_ref().map(|send| send.send(job));
    if !matches!(sent, Some(Ok(()))) {
        tracing::debug!("a matter's place could not be read for its count");
    }
}

/// `matter_opened` for `OpenMatter`: the folder's papers are listed on the matter's worker, as
/// `ListPapers` lists them, and the event counts when the listing answers. Nothing is listed
/// while counts are off or absent, and the Scratch matter is not counted (it is no folder).
fn matter_opened(actor: &mut Actor, matter: MatterId) {
    let Some(counts) = counting(actor) else {
        return;
    };
    let Some(folder) = actor
        .matters
        .get(&matter)
        .filter(|entry| entry.row.kind == clauseo_protocol::MatterKind::Folder)
        .map(|entry| PathBuf::from(&entry.row.folder_path))
    else {
        return;
    };
    let (reply, answer) = oneshot::channel();
    super::papers::list(actor, matter, reply);
    let counted = actor.spawner.spawn_local(async move {
        match answer.await {
            Ok(Ok(QueryResult::Papers { papers, .. })) => {
                let count = u64::try_from(papers.len()).unwrap_or(u64::MAX);
                bump_matter(
                    counts,
                    MatterKind::Folder,
                    Papers::from_count(count),
                    folder,
                );
            }
            other => tracing::debug!(?other, "a matter's papers were not listed for its count"),
        }
    });
    if let Err(error) = counted {
        tracing::debug!(%error, "the matter's count could not wait for its listing");
    }
}

/// The counts' agent for `thread`'s agent: Claude Code or Codex (`chatgpt`). Any other agent id
/// (`fake-agent`, a firm's own) has no value in the schema and is not counted.
fn agent(actor: &Actor, thread: ThreadId) -> Option<Agent> {
    let kind = actor
        .threads
        .get(&thread)
        .and_then(|entry| AgentKind::of(&entry.row.agent));
    match kind {
        Some(AgentKind::Claude) => Some(Agent::Claude),
        Some(AgentKind::Codex) => Some(Agent::Chatgpt),
        None => {
            tracing::debug!(%thread, "an agent with no usage-count value is not counted");
            None
        }
    }
}

/// `question_sent`: the prompt's commit is durable, with how it went.
pub(super) fn question_sent(actor: &Actor, thread: ThreadId, delivery: Delivery) {
    if let Some(agent) = agent(actor, thread) {
        bump(actor, Event::QuestionSent(QuestionSent { agent, delivery }));
    }
}

/// How a turn ended, as `turn::end::finish` reads it: when it started and when its first
/// answer text came (the host's monotonic clock), its stop, and its failure's code.
pub(super) struct Ended {
    pub(super) started: Duration,
    pub(super) first_words: Option<Duration>,
    pub(super) now: Duration,
    pub(super) stop: StopReason,
    pub(super) failure: Option<ErrorCode>,
}

/// `answer_ended`, from the one place every turn ends (`turn::end::finish`).
pub(super) fn answer_ended(actor: &Actor, thread: ThreadId, ended: &Ended) {
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    let took = |at: Duration| {
        let ms = at.saturating_sub(ended.started).as_millis();
        Took::from_millis(u64::try_from(ms).unwrap_or(u64::MAX))
    };
    bump(
        actor,
        Event::AnswerEnded(AnswerEnded {
            agent,
            outcome: outcome(ended.stop, ended.failure),
            took: took(ended.now),
            first_words: ended.first_words.map(took),
        }),
    );
}

/// The counts' outcome for a turn's stop. A turn that failed because the agent needs a sign-in
/// is `signin_expired`, whatever its stop; `offline` has no signal in the host yet.
pub(super) fn outcome(stop: StopReason, failure: Option<ErrorCode>) -> AnswerOutcome {
    if failure == Some(ErrorCode::AgentAuthRequired) {
        return AnswerOutcome::SigninExpired;
    }
    match stop {
        StopReason::EndTurn => AnswerOutcome::Done,
        StopReason::Cancelled => AnswerOutcome::StoppedByLawyer,
        StopReason::AtLimit => AnswerOutcome::Limit,
        StopReason::MaxTokens | StopReason::MaxTurnRequests | StopReason::Refusal => {
            AnswerOutcome::AgentStopped
        }
        StopReason::Failed | StopReason::Unknown => AnswerOutcome::Error,
    }
}

/// `feature_used`, once the operation's log is durable.
pub(super) fn feature_used(actor: &Actor, feature: Feature) {
    bump(actor, Event::FeatureUsed(FeatureUsed { feature }));
}

/// `setting_changed` for each `ui` value a write changed. `ui.density` has no value in the
/// schema, and the window no longer sets it.
pub(super) fn ui_changed(actor: &Actor, before: &settings::UiSettings) {
    let after = &actor.effective.settings().ui;
    let mut changed = Vec::new();
    if before.theme != after.theme {
        changed.push(SettingChanged::Theme(theme(after.theme)));
    }
    if before.font_size != after.font_size
        && let Ok(size) = TextSize::try_from(u64::from(after.font_size.get()))
    {
        changed.push(SettingChanged::TextSize(size));
    }
    if before.language != after.language {
        changed.push(SettingChanged::Language(language(after.language)));
    }
    if before.lite != after.lite {
        changed.push(SettingChanged::LiteMode(lite(after.lite)));
    }
    if before.motion != after.motion {
        changed.push(SettingChanged::ReducedMotion(motion(after.motion)));
    }
    for setting in changed {
        bump(actor, Event::SettingChanged(setting));
    }
}

/// `setting_changed` for `privacy.crash_reports` when a write changed it. Counts' own switch is
/// never counted: off sends nothing.
pub(super) fn privacy_changed(actor: &Actor, before: &settings::PrivacySettings) {
    let after = actor.effective.settings().privacy.crash_reports;
    if before.crash_reports != after {
        bump(
            actor,
            Event::SettingChanged(SettingChanged::CrashReports(after)),
        );
    }
}

fn theme(theme: settings::Theme) -> Theme {
    match theme {
        settings::Theme::System => Theme::System,
        settings::Theme::Light => Theme::Light,
        settings::Theme::Dark => Theme::Dark,
    }
}

fn language(language: settings::Language) -> Language {
    match language {
        settings::Language::En => Language::En,
        settings::Language::Hi => Language::Hi,
    }
}

fn lite(lite: settings::Lite) -> LiteMode {
    match lite {
        settings::Lite::Automatic => LiteMode::Automatic,
        settings::Lite::On => LiteMode::On,
        settings::Lite::Off => LiteMode::Off,
    }
}

fn motion(motion: settings::Motion) -> ReducedMotion {
    match motion {
        settings::Motion::System => ReducedMotion::System,
        settings::Motion::Reduce => ReducedMotion::Reduce,
        settings::Motion::Full => ReducedMotion::Full,
    }
}
