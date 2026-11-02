# 0041: Web searches and pages in the thread log

## Context

An agent can search the web and open pages while it works on a lawyer's question. An answer that
rests on those pages should show its sources. S26 records each web search and each page the agent
opens as a thread event, so the sources can be read back from the log.

This record covers the two events only: what they hold, how a page's opening is named, and why the
agent is named by registry id. The sources list that reads them is added by a later S26 PR.

## Decision

Two thread events, both from the agent (acp), both additive under
[0002](0002-event-schema.md). `schema_version` stays 1, and the reducer ignores them for now.

- **`web_search_recorded`**, a `WebSearchRecorded` payload:
  - `thread` (`ThreadId`), `turn` (`TurnId`), `tool_call_id` (`ToolCallId`): where the search ran
    and the tool call that ran it.
  - `agent` (`AgentId`): the agent that ran the search, by its registry id.
  - `query` (text): the query the agent sent.
  - `results` (a list of `WebResult`, in the order the agent got them): each with `title` and
    `url`. The list may be empty.
  - `at` (`Timestamp`): when it was recorded.
- **`page_visited`**, a `PageVisited` payload:
  - `thread`, `turn`, `tool_call_id`: as above, for the tool call that opened the page.
  - `url` (text): the page's address.
  - `title` (text, optional): the title, when the agent reported one. Left out of the JSON when
    there is none.
  - `at` (`Timestamp`).
  - `via` (`PageVia`): how the page was opened.
- **`PageVia`** is an open unit enum (`open_unit_enum!`, with a row in 0002's open list), with
  three values: `WebFetch` (the agent's web fetch tool), `CodexOpenPage` (Codex's open-page tool)
  and `Browser`. A value it does not know decodes as `Unknown`.

Each event has its row in 0002's event table, and the three `PageVia` values are in the frozen
corpus under `corpus/added/`, as the rules for added kinds say.

### Sources list

Added by a later S26 PR, which reads these two events.

## Deviations

- **`agent` is `AgentId`, not `AgentKind`.** The S26 brief named `agent: Option<AgentKind>`. The code
  uses `agent: AgentId`, the thread's agent registry id. Reason: [0002](0002-event-schema.md)
  requires every enum reachable from a logged type to be open. `AgentKind` is closed, and opening it
  would ripple through the `limits` crate, where it is a map key. `AgentKind::of` stays the one place
  an id becomes a kind, so a reader that wants the kind calls it.

## Consequences

The log now holds what the agent searched and opened, with the tool call and turn that did it, so a
later view can show sources from the log alone. The events are additive: an older reader sees them
as unknown kinds, and a newer one reads them with no migration and no snapshot change. What a later
change must respect: the fields are now part of the log, so a rename or a removed field needs its own
record under 0002, and a new `PageVia` value needs its row and its corpus entry. Until the sources
list lands, nothing shows these events to the lawyer.

## Links

- [0002: Event schema](0002-event-schema.md): the rules for additive kinds, open enums and the
  corpus.

## Status

proposed

## Date

2026-11-02
