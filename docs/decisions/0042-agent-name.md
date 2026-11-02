# 0042: The agent's own name in the thread header

## Context

When an agent of no known kind fails, the room names it by its settings id ("gemini-cli didn't
start …"). The agent's display name, `initialize`'s `agentInfo.title`, was dropped by `acp`. It
is there as soon as `initialize` answers, so it can be shown before an `auth_required` raised at
`session/new`. Only display needs it: the agent is still picked and identified by its settings id.

## Decision

- `acp` keeps `agentInfo.title` as `AgentInfo.title` (`None` when absent).
- A new thread-scoped kind, `agent_named { agent_name }` (`AgentNamed`), is logged by the host
  when `initialize` answers, before `session/new`, `session/resume` or the replay read. It is
  logged only when the cleaned title is present and differs from the name the thread's header
  holds, so a restart with the same agent logs nothing. The host cleans the title: trimmed,
  inner whitespace runs collapsed to one space, control characters and format characters (general
  category Cf, from a fixed table in `agent/title.rs`) dropped, at most 64 characters, and empty
  means no event. The Cf table includes the bidi embedding, override and isolate characters
  (U+202A to U+202E, U+2066 to U+2069). The zero width joiner and non-joiner (U+200D, U+200C) are
  kept, since scripts and emoji need them.
- A push or send refused at once is logged and the session goes on without the name. A store
  write that fails later is a store failure like any other commit's: it fails the launching turn.
- `HeaderEntry` gains `agent_name: Option<CompactString>`, set from the last `AgentNamed`. It is
  left out when `None`, and a header without it reads as `None`. When the thread's stream is
  unloaded, the header the actor keeps takes the name as the event is sent, so a second start
  does not log it again.
- The matter fold ignores the event. `REDUCER_SCHEMA_VERSION` goes from 3 to 4: `HeaderEntry`
  gained a field and the thread fold gained an arm, so a snapshot written at 3 holds a header
  without the name for a log that has one. Snapshots at 3 are rebuilt from events once, at their
  next reopen. The log's `schema_version` does not change: old logs have no `agent_named`, and a
  header without the field reads as `None`.

## Consequences

- The event is additive under the freeze rules: a new kind whose payload is a new struct with one
  required field. Its frozen entries are `corpus/added/agent_named.*`; the code owner approves it.
- The room still shows the settings id. Wiring the room to `agent_name` is a later step.
- An older reader skips `agent_name` in the header and ignores the unknown kind's event
  through its catch-all, so a log written by this version reads back in an older one.
- Any later change to the title rules (the host's cleaning) changes only what is written from
  now on; the logged names stay as they were written.
- The actor's kept header takes the name when the event is sent, before the store has it. A store
  write that then fails leaves that header claiming a name the log lacks, until the stream
  reloads from the log.

## Status

proposed

## Date

2026-11-03
