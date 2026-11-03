# 0027: limits in `host`

## Context

S13 built `limits` (0014): the readers and a pure model that takes inputs and `now` and returns
actions. Nothing ran it. R16 puts it in the host, which starts reads, feeds in live signals, turn
ends and pending questions, keeps each account's last `AccountLimits` in `store`, and publishes
the limits and the view. A turn stopped at the limit with no reset time in its signal needs that
time from a read, and 0002 lets a `turn_stopped_at_limit` follow its turn's `turn_ended` at most
once, so the end commit cannot carry one when the read is still to come. Without the AIR path,
codex-acp answers a limit with JSON-RPC error -32603 and prose, and Clauseo never parses prose.
A limit is not an error, so nothing here adds an `ErrorCode` (0003). R16 adds no UI and no
user-facing words: U5 replaced R12's window, and U12 builds the limit screens.

## Decision

- **Protocol additions**, all additive: `HostEvent::LimitsViewChanged(LimitsView)`, wire key
  `limits_view_changed`, clear of 0012's reserved keys; `AgentLimitsView.next_read_at:
  Option<Timestamp>` (the next read the model has planned, in stage B only a shown reset, from
  `Limits::next_read_at`) and `AgentLimitsView.provider: Option<BillingProvider>` (set for pay as
  you go, from `Billing::PayAsYouGo { via }`), both `#[serde(default)]`; `HostCommand::RefreshLimits
  { agent: Option<AgentKind>, why: RefreshWhy }` with `RefreshWhy::{Shown, TryAgain}`. It answers
  `CommandOutcome::Done`, stores no receipt and logs nothing, so a focus event never writes.
- **`HostHandle::subscribe_limits(principal) -> Result<Subscription, HostError>`.** It returns
  the existing `Subscription` with `Topic::Limits`, checked by the new `Access::SubscribeLimits`.
  A subscriber gets the current `LimitsViewChanged` first. Each change then gives one
  `AccountLimitsChanged` per changed account, followed by one `LimitsViewChanged`. The first
  subscriber feeds `Input::Watching { any: true }`, and the last one to go feeds `any: false`. A
  lagged subscriber, once drained, gets the current view, as app subscribers do. Account limits
  belong to no stream, and the view needs the clock, so they get a topic of their own.
- **The reader.** `HostConfig::with_limits_reader(Arc<dyn ReadLimits>)` sets it. By default the
  host builds `ProcessReaders::new(&effective, Locations::agent_home("codex-acp"), launcher,
  ProcessTimer(clock))`, the same home R15 launches codex-acp with. The host never reads, writes
  or links `auth.json` and never calls codex-acp's `logout`; stage D links the lawyer's own login
  into that home (QD.4). Reads run on the actor's `LocalPool` and post their outcome back as an
  input, one `ReadDone` per `Read`, so the actor loop never waits on a reader. Nothing waits on a
  read at shutdown.
- **Start-up.** Stored `AccountLimits` go into `Limits::new` with the kinds (`AgentKind::of`) of
  the settings entries, then one read per agent. With no `claude-acp` or `codex-acp` entry there
  is no model, no read, and the view is empty.
- **Inputs.** A permission or elicitation recomputes, from state, whether that agent kind has any
  question pending, and feeds `Asking` only when the answer changes. An accepted URL elicitation
  counts as pending until the agent sends `elicitation/complete`, so it holds that agent kind's
  reads until then, the turn's end or the agent's exit: a stuck turn holds reads until it is
  cancelled. A turn end feeds `TurnEnded`,
  with `read_now` for an `AtLimit` stop. One host timer runs at `Limits::next_deadline()`,
  re-armed after every input. With no subscriber the model gives no deadline, so no timer is armed,
  and a fire that comes while nobody watches is ignored. The timer counts monotonic time, which
  stops while the computer sleeps, so every input first sweeps a deadline whose wall time passed.
- **Focus throttle.** `RefreshWhy::Shown` is throttled by S13's model rule alone: it reads only
  when the last read started 5 minutes ago or more (`SHOWN_AFTER_MS`, `policy/triggers.rs`). The
  host adds no second number. A throttled `Shown` publishes nothing.
- **R11 change 1, the close.** When a turn stops `AtLimit` and its limit signal has `resets_at:
  None`, the end commit holds `TurnEnded { AtLimit }` and no `TurnStoppedAtLimit`. The host asks
  for a read and, on the `ReadDone` of that agent's next read started after the stop, logs one
  `TurnStoppedAtLimit` in its own commit. Reads are numbered as they start; a read already
  running when the stop came may predate the limit, so its `ReadDone` decides nothing and the
  host asks for a read again (`TurnEnded { read_now: true }`), which keeps the model's one read
  per agent. `window` and `resets_at` come from `Limits::at_limit(agent, now)`, both `None`
  when it answers `None` or the read failed; `reason` comes from the turn's limit signal. With
  no such `ReadDone` within the agent kind's wait (`read_wait` in `actor/limits.rs`),
  `at_limit` as it stands then decides. Claude waits 30 s: its reader's own 25 s deadline and
  5 s more, so a slow read still gives the reset time. Codex waits 10 s, the Codex reader's
  deadline: a read that hits it answers `Failed(TimedOut)`, which decides from `at_limit` as it
  stands, as the end of the wait does. The stop is logged only once the end commit is durable,
  and stays pending, keeping the thread's stream loaded, until its own commit is; a failed
  commit is retried. A stop whose signal has a reset time keeps R11's single commit, then a
  read.
- **Shutdown and pending stops.** Shutdown decides every pending stop still waiting from
  `at_limit` as it stands (both `None` when it answers `None`), with the reason from the
  turn's limit signal, logs it, and keeps the actor up until each of those commits returns. A
  stop whose turn's end is not durable yet is logged once that end's commit is `Ok`, as at any
  time; a turn that stops during shutdown has its stop decided at once, with no read. Only a
  crash leaves a stop to the start-up repair.
- **The no-kind rule.** An agent id with no `AgentKind`, or a kind the model does not have, keeps
  the stop in the end commit, since no read will come.
- **R11 change 2, signals outside a turn.** A `LimitSignal::RateLimit` outside a turn feeds the
  model as `Input::Live`, not only `tracing`. The exception: signals replayed by `session/load`
  (`Origin::Replay`) never feed it, because they are old and the model would date them now.
- **Start-up repair** (`actor/turn/reconcile.rs`). Every step of a thread that stopped `AtLimit`
  with no `LimitStopEntry` for its turn gets one `TurnStoppedAtLimit { None, None, PlanWindow }`,
  each in a commit of its own. Every step, not only the last: once a turn's end is durable a new
  turn may start in the same thread while that turn's stop still waits, so after a crash an
  earlier turn may lack its card too. New turns are not held back for the stop. This is the one
  exception to "a turn with a `TurnEnded` is never touched". It runs only at start-up; once
  logged, the entry exists and the check is false, so it is idempotent across restarts, as 0011
  and #113 require of repair.
- **The Codex fallback.** Only for a `codex-acp` prompt that fails with `Error::Rpc`, outside
  shutdown. After step 6 of 0011's turn order the host feeds `TurnEnded { Codex, read_now: true }`
  once and holds the end commit for one Codex read, bounded host-side at 10 s (`CODEX_READ_WAIT`).
  Then if `at_limit` answers, the turn ends `AtLimit` with the stop from the `LimitHit` in the same
  commit; else it ends `TurnFailed { agent.protocol_error }` as before. Nothing goes to the model
  after the end commit. A shutdown during the hold decides at once.
- **The dependency.** The `host` row has listed `limits` since R15 (`ARCHITECTURE.md`); this
  record covers the model's use of it.
- **The bridge.** `open_stream` follows one `subscribe_limits` on each window's Channel. Its
  events count toward acks and the 256 KB pause like any other. As with 0019's app feed, they are
  never wrapped as `timed` and send no `stream_failed`, which names a `StreamId` that limits lack;
  a refused renewal, only once the host has stopped, stops following them. When the bridge renews
  its subscriptions (0012), the new one starts with the current view, so nothing is replayed. The
  window's native focus event dispatches `RefreshLimits { agent: None, why: Shown }` from Rust.
  The command allow-list admits `RefreshLimits`.

## Consequences

- Limit ticks never reach the log; `store` keeps the last `AccountLimits` per account.
- A stop whose turn's end never becomes durable (its retry failed too) is not logged in that run;
  the next start ends that turn, as 0011 says.
- The agent list and the reader are fixed at start-up, so a `claude-acp` or `codex-acp` entry
  added later gets limits after a restart.
- A Codex turn failing with a JSON-RPC error ends up to 10 s later than before.
- U12 builds the screens on `LimitsViewChanged`, `next_read_at` and `provider`; the settings page
  and meters dispatch `RefreshLimits { why: Shown }` when they open.
- A later change to the close must keep at most one `TurnStoppedAtLimit` per turn across the end
  commit, the deferred stop and the repair.

## Status

proposed

Amended 2026-11-03: since #185 (U11), only the `main` window's Channel follows limits; U12 opens
it for Settings when Plan and usage reads it ([0028](0028-settings-window.md)).

Amended 2026-11-03: U12 opened the limits for the Settings window: its Channel follows them as
`main`'s does, it may send `refresh_limits`, and its focus dispatches `Shown` from Rust
([0028](0028-settings-window.md)).

## Date

2026-11-02
