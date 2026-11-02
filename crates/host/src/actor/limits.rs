//! The host's part of `limits`: the [`Limits`] model, the reader, the limits subscribers, the
//! one limits timer and the last `asking` sent per agent kind. The rest of the actor calls in
//! here with one line each; every input goes through [`feed`], which gives the model the host
//! clock's wall time, carries out every [`Action`] it returns and re-arms the timer.
//!
//! - A read ([`Action::Read`]) runs as a task on the actor's pool and posts its outcome back as
//!   [`LimitsInput::Read`], which becomes exactly one `Input::ReadDone`. The loop never waits
//!   on a reader; dropping the actor drops the reads in flight.
//! - [`Action::Changed`] is stored (`Store::put_account_limits`, off the loop; a failure is
//!   only logged) and published as `AccountLimitsChanged`. The actions of one input publish
//!   each `AccountLimitsChanged`, then one `LimitsViewChanged` when any action changed the view.
//! - The timer waits for [`Limits::next_deadline`] on the host clock. It is replaced only when
//!   that time moves, and a fire carries the generation it was armed with, so a fire that raced
//!   a re-arm is ignored. The model gives no deadline while nobody watches, so with no limits
//!   subscriber no limits timer is armed.
//! - Subscribers follow `app.rs`'s rules (bounded channel, `try_send`, a full one is caught up
//!   with the current view once it drains). The first one turns `Watching` on, and its view is
//!   the first event it gets; a later one gets the current view alone; the last one to go turns
//!   `Watching` off.
//! - With no `claude-acp` or `codex-acp` entry in the settings there is no model: nothing reads,
//!   the view is empty and every input does nothing.
//! - A turn stopped at the limit with no reset time leaves a pending stop here ([`stop_later`]).
//!   The agent's next `ReadDone`, whichever read it is, logs one `TurnStoppedAtLimit` for each,
//!   filled from [`Limits::at_limit`] (nothing for a failed read). A pending stop lives in
//!   memory only: one lost to a restart is logged by start-up repair (`turn/reconcile.rs`).
//!   After every `ReadDone` of Codex, a Codex turn waiting on a read before it ends (the turn
//!   path's Codex fallback) is told.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use clauseo_acp::{LimitSignal, LimitStatus};
use clauseo_limits::{
    Action, Input as ModelInput, LimitHit, Limits, LiveSignal, LiveStatus, ProcessReaders,
    ReadLimits, ReadOutcome,
};
use clauseo_protocol::{
    AccountLimits, AgentId, AgentKind, HostEvent, LimitStopReason, LimitsView, Principal,
    RefreshWhy, ThreadId, Timestamp, TurnId, TurnStoppedAtLimit,
};
use clauseo_settings::Effective;
use futures::future::AbortHandle;

use super::Actor;
use super::app::{AppSub, encode};
use super::input::{Input, Reply, SubId};
use crate::clock::{Clock, ProcessTimer};
use crate::encode::EncodedEvent;
use crate::handle::Subscription;
use crate::principal::{Access, authorize};

/// The agent id whose home the host launches Codex with, as `CODEX_HOME`
/// (`turn/launch_checks.rs` takes `Locations::agent_home` of the thread's agent id, which is
/// this one for every Codex thread).
const CODEX_AGENT: &str = "codex-acp";

/// What the actor holds for limits.
pub(crate) struct LimitsHost {
    /// The model; `None` while no settings entry has a kind.
    model: Option<Limits>,
    /// The agents with a settings entry, the ones the model knows.
    agents: Vec<AgentKind>,
    reader: Arc<dyn ReadLimits>,
    /// The limits subscribers, in the order they came.
    subs: Vec<AppSub>,
    /// The model was told someone watches.
    watching: bool,
    /// The armed timer and the deadline it waits for.
    timer: Option<(AbortHandle, Timestamp)>,
    /// The generation of the last timer armed; a fire of another one is stale.
    generation: u64,
    /// The last `asking` sent per agent; none sent counts as `false`.
    asking: BTreeMap<AgentKind, bool>,
    /// Turns stopped at the limit whose `TurnStoppedAtLimit` waits for their agent's next read.
    stops: Vec<PendingStop>,
}

/// A turn stopped at the limit with no reset time, waiting for its agent's next read.
struct PendingStop {
    agent: AgentKind,
    thread: ThreadId,
    turn: TurnId,
    /// From the turn's limit signal.
    reason: LimitStopReason,
}

/// What the actor's own tasks and the limits subscriptions post back.
pub(crate) enum LimitsInput {
    /// A read the model asked for ended.
    Read {
        agent: AgentKind,
        outcome: ReadOutcome,
    },
    /// The limits timer of `generation` fired.
    Deadline { generation: u64 },
    /// A limits subscription drained its channel after the actor found it full.
    Drained { id: SubId },
    /// A limits subscription was dropped.
    Unsubscribed { id: SubId },
}

impl LimitsHost {
    /// No model yet; [`start`] makes it once the actor exists.
    pub(crate) fn new(reader: Arc<dyn ReadLimits>) -> Self {
        Self {
            model: None,
            agents: Vec::new(),
            reader,
            subs: Vec::new(),
            watching: false,
            timer: None,
            generation: 0,
            asking: BTreeMap::new(),
            stops: Vec::new(),
        }
    }
}

/// The default reader: `ProcessReaders` over the settings, the launcher, the host clock and
/// the Codex home the host launches `codex-acp` with.
pub(crate) fn default_reader(
    effective: &Effective,
    launcher: &Arc<dyn clauseo_process::Launcher>,
    clock: &Arc<dyn Clock>,
) -> Arc<dyn ReadLimits> {
    let codex_home = effective.locations().agent_home(CODEX_AGENT);
    Arc::new(ProcessReaders::new(
        effective,
        codex_home,
        Arc::clone(launcher),
        ProcessTimer(Arc::clone(clock)),
    ))
}

/// Start-up: the model from the cached limits and the agents with a settings entry, and the
/// reads it asks for (one per agent). Nothing waits on them.
pub(super) fn start(actor: &mut Actor, cached: Vec<AccountLimits>) {
    let mut agents: Vec<AgentKind> = Vec::new();
    for id in actor.effective.settings().agents.keys() {
        let kind = AgentId::new(id).ok().and_then(|id| AgentKind::of(&id));
        if let Some(kind) = kind
            && !agents.contains(&kind)
        {
            agents.push(kind);
        }
    }
    if agents.is_empty() {
        return;
    }
    let now = actor.now().wall;
    let (model, actions) = Limits::new(cached, &agents, now);
    actor.limits.model = Some(model);
    actor.limits.agents = agents;
    carry_out(actor, actions, now);
    rearm(actor, now);
}

/// One input from the actor's tasks or a limits subscription.
pub(super) fn on_input(actor: &mut Actor, input: LimitsInput) {
    match input {
        LimitsInput::Read { agent, outcome } => {
            let failed = matches!(outcome, ReadOutcome::Failed(_));
            feed(actor, ModelInput::ReadDone { agent, outcome });
            read_done(actor, agent, failed);
        }
        LimitsInput::Deadline { generation } => {
            if generation != actor.limits.generation {
                return;
            }
            actor.limits.timer = None;
            feed(actor, ModelInput::Deadline);
        }
        LimitsInput::Drained { id } => on_drained(actor, id),
        LimitsInput::Unsubscribed { id } => {
            actor.limits.subs.retain(|sub| sub.id() != id);
            let now = actor.now().wall;
            unwatch_if_alone(actor, now);
            rearm(actor, now);
        }
    }
}

/// `subscribe_limits`: the principal check, then a subscriber whose first event is the current
/// `LimitsViewChanged`.
pub(super) fn subscribe(actor: &mut Actor, principal: &Principal, reply: Reply<Subscription>) {
    if let Err(error) = authorize(principal, Access::SubscribeLimits) {
        let _gone = reply.send(Err(error));
        return;
    }
    let id = super::subs::next_sub_id(actor);
    let (subscription, sink) = Subscription::limits(actor.post.clone(), id);
    if reply.send(Ok(subscription)).is_err() {
        return;
    }
    let mut sub = AppSub::new(id, sink);
    if actor.limits.model.is_some() && !actor.limits.watching {
        // The first subscriber: `Watching` answers with the view, its first event.
        actor.limits.subs.push(sub);
        actor.limits.watching = true;
        feed(actor, ModelInput::Watching { any: true });
        return;
    }
    let now = actor.now().wall;
    if let Some(view) = view_event(actor, now)
        && sub.offer(&view)
    {
        actor.limits.subs.push(sub);
    }
}

/// `RefreshLimits`: one model input. A `Shown` the model throttles returns no action, so
/// nothing is built or published.
pub(crate) fn refresh(actor: &mut Actor, agent: Option<AgentKind>, why: RefreshWhy) {
    let input = match why {
        RefreshWhy::Shown => ModelInput::Shown { agent },
        RefreshWhy::TryAgain => ModelInput::TryAgain { agent },
    };
    feed(actor, input);
}

/// A rate-limit signal of `thread`'s agent, in a turn or outside one: `Input::Live` for the
/// agent's kind (an id with no kind has no limits).
pub(crate) fn live(actor: &mut Actor, thread: ThreadId, signal: &LimitSignal) {
    let LimitSignal::RateLimit {
        window,
        status,
        used_percent,
        resets_at,
    } = signal
    else {
        return;
    };
    let Some(agent) = kind_of(actor, thread) else {
        return;
    };
    let status = match status {
        LimitStatus::Allowed => LiveStatus::Allowed,
        LimitStatus::Warning => LiveStatus::Warning,
        LimitStatus::Rejected => LiveStatus::Rejected,
    };
    let signal = LiveSignal {
        window: window.clone(),
        status,
        used_percent: *used_percent,
        resets_at: *resets_at,
    };
    feed(actor, ModelInput::Live { agent, signal });
}

/// The pending permissions or elicitations changed somewhere: for each agent kind, whether any
/// thread of that kind has one pending, sent as `Input::Asking` when it differs from the last
/// value sent.
pub(crate) fn asking_changed(actor: &mut Actor) {
    if actor.limits.model.is_none() {
        return;
    }
    for agent in actor.limits.agents.clone() {
        let asking = super::turn::asks_pending(actor, agent);
        let last = actor.limits.asking.get(&agent).copied().unwrap_or(false);
        if asking != last {
            actor.limits.asking.insert(agent, asking);
            feed(actor, ModelInput::Asking { agent, asking });
        }
    }
}

/// A turn of `thread` ended; `read_now` for a stop at the limit.
pub(crate) fn turn_ended_in(actor: &mut Actor, thread: ThreadId, read_now: bool) {
    if let Some(agent) = kind_of(actor, thread) {
        turn_ended(actor, agent, read_now);
    }
}

/// A turn of an `agent` thread ended: `Input::TurnEnded`. The seam for R16's Codex fallback,
/// which sends `read_now: true` once, before the end commit.
pub(crate) fn turn_ended(actor: &mut Actor, agent: AgentKind, read_now: bool) {
    feed(actor, ModelInput::TurnEnded { agent, read_now });
}

/// The kind of `thread`'s agent when the model has it, so a read of it will come; `None` for an
/// agent id with no kind, or with no model.
pub(crate) fn reads_for(actor: &Actor, thread: ThreadId) -> Option<AgentKind> {
    kind_of(actor, thread).filter(|agent| actor.limits.agents.contains(agent))
}

/// What the model says `agent` is at now, if at its limit.
pub(crate) fn at_limit(actor: &Actor, agent: AgentKind) -> Option<LimitHit> {
    let now = actor.now().wall;
    actor.limits.model.as_ref()?.at_limit(agent, now)
}

/// A turn of `thread` stopped at the limit with no reset time: its `TurnStoppedAtLimit` waits
/// for `agent`'s next read, which this asks for (`TurnEnded { read_now: true }`).
pub(crate) fn stop_later(
    actor: &mut Actor,
    agent: AgentKind,
    thread: ThreadId,
    turn: TurnId,
    reason: LimitStopReason,
) {
    actor.limits.stops.push(PendingStop {
        agent,
        thread,
        turn,
        reason,
    });
    turn_ended(actor, agent, true);
}

/// A read of `agent` is done and the model has its outcome: one `TurnStoppedAtLimit` for each
/// of the agent's pending stops (no window or reset after a failed read), then the turn path
/// hears of it.
fn read_done(actor: &mut Actor, agent: AgentKind, failed: bool) {
    let hit = if failed { None } else { at_limit(actor, agent) };
    let (due, kept): (Vec<PendingStop>, Vec<PendingStop>) = std::mem::take(&mut actor.limits.stops)
        .into_iter()
        .partition(|stop| stop.agent == agent);
    actor.limits.stops = kept;
    for stop in due {
        let stopped = TurnStoppedAtLimit {
            turn: stop.turn,
            window: hit.as_ref().and_then(|hit| hit.window.clone()),
            resets_at: hit.as_ref().and_then(|hit| hit.resets_at),
            reason: stop.reason,
        };
        super::turn::log_limit_stop(actor, stop.thread, stopped);
    }
    super::turn::limits_read(actor, agent);
}

/// The kind of `thread`'s agent, if it has one.
fn kind_of(actor: &Actor, thread: ThreadId) -> Option<AgentKind> {
    actor
        .threads
        .get(&thread)
        .and_then(|entry| AgentKind::of(&entry.row.agent))
}

/// Gives the model `input` at the host clock's wall time, carries out what it answers and
/// re-arms the timer. Does nothing with no model.
fn feed(actor: &mut Actor, input: ModelInput) {
    let now = actor.now().wall;
    let Some(model) = actor.limits.model.as_mut() else {
        return;
    };
    let actions = model.input(input, now);
    if actions.is_empty() {
        rearm(actor, now);
        return;
    }
    carry_out(actor, actions, now);
    unwatch_if_alone(actor, now);
    rearm(actor, now);
}

/// Carries out the model's actions: reads start, changed limits are stored and published,
/// then one view goes out when anything changed it.
fn carry_out(actor: &mut Actor, actions: Vec<Action>, now: Timestamp) {
    let mut view_changed = false;
    for action in actions {
        match action {
            Action::Read(agent) => start_read(actor, agent),
            Action::Changed(limits) => {
                store(actor, limits.clone());
                publish(actor, &HostEvent::AccountLimitsChanged(limits));
                view_changed = true;
            }
            Action::ViewChanged => view_changed = true,
        }
    }
    if view_changed && !actor.limits.subs.is_empty() {
        let view = view_event(actor, now);
        if let Some(view) = view {
            actor.limits.subs.retain_mut(|sub| sub.offer(&view));
        }
    }
}

/// Starts one read on the pool; its outcome comes back as [`LimitsInput::Read`].
fn start_read(actor: &Actor, agent: AgentKind) {
    let read = actor.limits.reader.read(agent);
    actor.spawn_post(async move {
        let outcome = read.await;
        Input::Limits(LimitsInput::Read { agent, outcome })
    });
}

/// Keeps `limits` as the last value for its account, off the loop.
fn store(actor: &Actor, limits: AccountLimits) {
    let store = actor.store.clone();
    let spawned = futures::task::LocalSpawnExt::spawn_local(&actor.spawner, async move {
        if let Err(error) = store.put_account_limits(limits).await {
            tracing::warn!(%error, "account limits were not stored");
        }
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "the host pool refused a task");
    }
}

/// Sends `event` to every limits subscriber, serialised once.
fn publish(actor: &mut Actor, event: &HostEvent) {
    if actor.limits.subs.is_empty() {
        return;
    }
    match encode(event) {
        Ok(encoded) => actor.limits.subs.retain_mut(|sub| sub.offer(&encoded)),
        Err(error) => tracing::error!(%error, "a limits event did not serialise"),
    }
}

/// The current `LimitsViewChanged`, serialised: the model's view at `now`, or an empty one
/// with no model.
fn view_event(actor: &Actor, now: Timestamp) -> Option<EncodedEvent> {
    let view = actor
        .limits
        .model
        .as_ref()
        .map_or_else(LimitsView::default, |model| model.view(now));
    encode(&HostEvent::LimitsViewChanged(view))
        .inspect_err(|error| tracing::error!(%error, "a limits event did not serialise"))
        .ok()
}

/// A lagged subscriber drained its channel: it gets the current view.
fn on_drained(actor: &mut Actor, id: SubId) {
    let Some(index) = actor.limits.subs.iter().position(|sub| sub.id() == id) else {
        return;
    };
    let now = actor.now().wall;
    let events: Vec<EncodedEvent> = view_event(actor, now).into_iter().collect();
    let stays = actor
        .limits
        .subs
        .get_mut(index)
        .is_none_or(|sub| sub.catch_up(&events));
    if !stays {
        actor.limits.subs.remove(index);
        unwatch_if_alone(actor, now);
        rearm(actor, now);
    }
}

/// The last subscriber went: the model hears that nobody watches.
fn unwatch_if_alone(actor: &mut Actor, now: Timestamp) {
    if !actor.limits.watching || !actor.limits.subs.is_empty() {
        return;
    }
    actor.limits.watching = false;
    let actions = match actor.limits.model.as_mut() {
        Some(model) => model.input(ModelInput::Watching { any: false }, now),
        None => return,
    };
    carry_out(actor, actions, now);
}

/// Arms the timer at the model's next deadline, replacing the armed one only when that time
/// moved, or cancels it when there is none.
fn rearm(actor: &mut Actor, now: Timestamp) {
    let deadline = actor.limits.model.as_ref().and_then(Limits::next_deadline);
    let armed = actor.limits.timer.as_ref().map(|(_, at)| *at);
    if deadline == armed {
        return;
    }
    if let Some((timer, _)) = actor.limits.timer.take() {
        timer.abort();
    }
    let Some(at) = deadline else {
        return;
    };
    actor.limits.generation = actor.limits.generation.wrapping_add(1);
    let generation = actor.limits.generation;
    let millis = at.as_millis().saturating_sub(now.as_millis()).max(0);
    let delay = Duration::from_millis(u64::try_from(millis).unwrap_or(0));
    let handle = actor.post_after(delay, Input::Limits(LimitsInput::Deadline { generation }));
    actor.limits.timer = Some((handle, at));
}
