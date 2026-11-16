# 0052: Idle stop and resume in the host

## Context

A live agent holds memory while it waits for the next question. G3 measured this on macOS with
Node 24.22.0 (`measurements/g3-agent-cost-and-resume.md`, #427 and #431). After one turn and
600 s idle, claude-agent-acp holds about 194 MB and codex-acp about 99 MB (item 3). Resuming
the session with `session/load` takes 508 ms for Claude and 391 ms for Codex (item 4). Both
recalled a word after a normal stop and after a SIGKILL mid-turn. The synthesis planned a stop
after 10 minutes idle (§2.2) and at most 2 live agents on an 8 GB machine (§4). The protocol
already had `DetachReason::IdleStopped` and `ThreadStatus::IdleStopped`. Idle stop was planned
for the `agents` crate. The host owns agent processes and thread status, and `host` never
depends on `agents` (ruling 27), so the stop belongs in `host`.

## Decision

- **When the host stops an agent.** An agent is stopped after 300 s with nothing pending (Rohan,
  2026-11-06, on G3's proposal; `budgets.toml` `agent_idle_stop`). "Pending" means any of these:
  - a current turn, or one whose end is not durable yet;
  - a queued prompt (held or not), a steer out, or a removal in flight;
  - a turn held across an expired sign-in;
  - a permission request or an elicitation, including an accepted URL elicitation that is not
    complete yet;
  - a launch or a session load in progress, or work waiting for one;
  - a config or mode call in flight;
  - a case-law relay call in flight, in a turn or between turns. The relay reports each message
    it forwards as it starts and ends (`RelayEvents::call`), and the actor counts it on the
    agent of that launch;
  - a `SendPrompt`, `SetConfigOption` or `SetMode` for the thread that has arrived but is still
    waiting for its receipt check.

  The limit lives in the host config (`HostConfig::with_idle_limit`, default `IDLE_LIMIT`).
  Tests shorten it, and `Duration::MAX` turns the stop off.
- **The idle clock.** Each agent's clock starts when the last pending item ends. After every
  input the actor handles, it brings each live agent's clock up to date (`actor/turn/idle.rs`).
  Any event from the agent, an update or a request, in a turn or between turns, starts its
  clock again. An agent still streaming after its turn's end is stopped a full limit after its
  last update.
  The actor keeps one timer, for the earliest idle agent's deadline, and no timer while no
  agent is idle. When the timer fires, the actor checks every agent again at that moment,
  inside the actor, and stops only the agents that still have nothing pending. A prompt
  handled before the fire, in the same tick, keeps its agent alive.
- **How it stops.** The stop uses `process`'s shutdown ladder: stdin is closed, then after 2 s
  SIGTERM, then after 3 s SIGKILL on Unix; on Windows stdin is closed and then the job ends. The
  stop cause is `StopCause::IdleStopped`, so `Ended` logs
  `AgentSessionDetached { reason: IdleStopped }`. The lifecycle log line `agent idle-stopped`
  gives the thread, how long it was idle and why it was stopped. On macOS (G3 item 4)
  claude-agent-acp exits when stdin closes, and codex-acp ignores that and needs SIGTERM after
  the 2 s wait. The Windows step is pending G3 item 4. With `fake_agent`, the tests check each
  step: stdin close, and SIGTERM for an agent that ignores it.
- **How it resumes.** The next question on the thread relaunches the agent through the normal
  launch path; nothing is added for idle stop:
  - if the agent offers `loadSession` and the thread has a stored id, the host reads the whole
    log and sends `session/load`, and drops the replayed updates the log already holds;
  - otherwise, if the agent offers `sessionCapabilities.resume`, it sends `session/resume`;
  - otherwise it sends `session/new` and logs `CONTEXT_NOT_RESTORED`'s notice.

  A `session/load` that fails also falls back to `session/new` with that notice. A prompt that
  arrives while the stop is running waits for the agent's end, then relaunches it. The thread
  stays `Starting` through the detach (see the resume cue below).
- **A sign-in stop during an idle stop.** When `stop_agents_for` or `restart_agents` comes while
  an agent is already stopping for idling, the stop becomes the sign-in stop. The agent detaches
  once, with `Closed`, not `IdleStopped`, so the thread shows the sign-in hold.
- **The cap.** On a machine with 8 GB of memory or less, the live-agents cap applies
  (`HostConfig::with_live_agents`, default `LIVE_AGENTS` = 2). The memory comes from
  `with_machine_memory`, which desktop fills. Unknown memory counts as small, as it does for
  the browser slots. When a launch finds 2 agents of other threads live or launching, the host
  first idle-stops the one that has been idle longest with nothing pending, if it has been idle
  at least `cap_min_idle` (`HostConfig::with_cap_min_idle`, default `CAP_MIN_IDLE` = 60 s).
  That stop logs the same `IdleStopped`. The minimum keeps a lawyer who moves between three
  threads from paying a stop and a resume at every switch. When no live agent qualifies, because
  each is busy or went idle less than 60 s ago, the new agent starts anyway and the host logs a
  warning. A config or mode change on an idle-stopped thread launches its agent under the same
  rule. This rule is provisional until G3 item 3 measures 2 and 3 live agents on the
  8 GB copy. Above 8 GB there is no cap in v1.
- **What the reducer folds.** `DetachReason::IdleStopped` folds into `ThreadStatus::IdleStopped`.
  If a turn is running, it becomes the status after the turn (`after_turn`). The next
  `AgentSessionAttached` changes `IdleStopped` back to the settled status and drops a stop
  waiting for a turn's end. The rail and the triage sweep treat `IdleStopped` as idle.
- **The resume cue.** The thread header's `resuming_since` is the timestamp of the
  `UserMessage` that moved an `IdleStopped` thread to `Starting`. The reducer derives it from
  logged events alone, so it survives a reload. When the prompt came during the stop, the thread
  is already `Starting` at the `IdleStopped` detach: it stays `Starting`, and the cue is the
  detach's timestamp. The next `AgentSessionAttached` clears it, and
  so does any status other than `Starting`. It is never set on a first start or after
  `AgentExited`. The app decides when to show it (`docs/ui-states.md`).
  `DetachReason::Restarting` keeps its own not-yet-folded arm (R18).

## Consequences

- An idle thread holds no agent memory after 5 minutes. Its next question pays a relaunch plus a
  load, about half a second on macOS (G3 items 1 and 4). The by-hand row in R20b's PR compares
  the time from send to the first update with these figures.
- Any new kind of pending work must be added to `idle::pending` (or to `queue::waits` for the
  queue), or the idle stop could end an agent in the middle of that work. Each check there has
  a test of its own that fails without it.
- A relay call is counted only when the connectors are wired (`HostConfig::with_connectors`),
  because the relay's events reach the actor through that sink. The app always wires both.
- Integration tests run with the idle stop and the cap off (`tests/host/idle_stop.rs` `off`), so
  a test that moves the clock by hours does not lose its agent. The idle stop's own tests turn
  them on.
- No wire type was added.
- Pending, and not blocking: Windows resume and stop step (G3 item 4), the 8 GB rows for
  `live_agents_8gb` and the third-agent rule (G3 item 3), and the "waking agent" words (app
  track).

## Status

accepted

## Date

2026-11-06
