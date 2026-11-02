# 0028: The agent's own name in the thread header

## Context

When an agent of no known kind fails, the room names it by its settings id ("gemini-cli didn't
start …"). The agent's display name, `initialize`'s `agentInfo.title`, was dropped by `acp`. It
is there as soon as `initialize` answers, so it can be shown before an `auth_required` raised at
`session/new`. Only display needs it: the agent is still picked and identified by its settings id.

## Decision

- `acp` keeps `agentInfo.title` as `AgentInfo.title` (`None` when absent).
- A new thread-scoped kind, `agent_named { agent_name }` (`AgentNamed`), is logged by the host
  when `initialize` answers, before `session/new`, `session/resume` or the replay read. It is
  logged only when the cleaned title is present and differs from the header's `agent_name`, so
  a restart with the same agent logs nothing. The host cleans the title: trimmed, control and
  bidi override or isolate characters dropped, inner whitespace collapsed, at most 64 characters,
  and empty means no event.
- `HeaderEntry` gains `agent_name: Option<CompactString>`, set from the last `AgentNamed`. It is
  left out when `None`, and a header without it reads as `None`.
- The matter fold ignores the event. Old logs have no `agent_named`, so they fold as before and
  no `schema_version` or `REDUCER_SCHEMA_VERSION` changes.

## Consequences

- The event is additive under the freeze rules: a new kind whose payload is a new struct with one
  required field. Its frozen entries are `corpus/added/agent_named.*`; the code owner approves it.
- The room still shows the settings id. Wiring the room to `agent_name` is a later step.
- An older reader skips `agent_name` in the header and ignores the unknown kind's event
  through its catch-all, so a log written by this version reads back in an older one.
- Any later change to the title rules (the host's cleaning) changes only what is written from
  now on; the logged names stay as they were written.

## Status

proposed

## Date

2026-11-03
