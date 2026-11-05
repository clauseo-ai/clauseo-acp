# 0041: Web searches and pages in the thread log

## Context

An agent can search the web and open pages while it works on a lawyer's question. An answer that
rests on those pages should show its sources. S26 records each web search and each page the agent
opens as a thread event, so the sources can be read back from the log.

This record covers the two events only: what they hold, how a page's opening is named, why the
agent is named by registry id, what the log keeps of a URL and how long a field may be. The sources
list that reads them is added by a later S26 PR.

## Decision

Two thread events, both from the agent (acp), both additive under
[0002](0002-event-schema.md). `schema_version` stays 1, and the reducer ignores them for now.

The thread and the time are not in the payloads. The envelope already carries them: `stream` is the
thread, and `ts` is when the event was committed. A payload that repeated them would hold two copies
that could disagree.

- **`web_search_recorded`**, a `WebSearchRecorded` payload:
  - `turn` (`TurnId`), `tool_call` (`ToolCallId`): the turn the search ran in, and the tool call
    that ran it. The name and type of `tool_call` are those of `tool_call_started` and
    `tool_call_updated`.
  - `agent` (`AgentId`): the agent that ran the search, by its registry id.
  - `query` (text, at most `MAX_SEARCH_QUERY_CHARS` = 1,000 characters): the query the agent sent.
  - `results` (a list of `WebResult`, in the order the agent got them, at most
    `MAX_SEARCH_RESULTS` = 50): each with `title` and `url`. The list may be empty.
- **`page_visited`**, a `PageVisited` payload:
  - `turn`, `tool_call`: as above, for the tool call that opened the page.
  - `url` (text, at most `MAX_URL_CHARS` = 4,096 characters, after the URL rule below): the page's
    address.
  - `title` (text, optional, at most `MAX_WEB_TITLE_CHARS` = 300 characters): the title, when the
    agent reported one. Left out of the JSON when there is none.
  - `via` (`PageVia`): how the page was opened.
- **`WebResult`**: `title` (at most `MAX_WEB_TITLE_CHARS` = 300 characters) and `url` (at most
  `MAX_URL_CHARS` = 4,096 characters, after the URL rule).
- **`PageVia`** is an open unit enum (`open_unit_enum!`, with a row in 0002's open list), with
  three values: `WebFetch` (the agent's web fetch tool), `CodexOpenPage` (Codex's open-page tool)
  and `Browser`. A value it does not know decodes as `Unknown`.

Each event has its row in 0002's event table, and the three `PageVia` values are in the frozen
corpus under `corpus/added/`, as the rules for added kinds say.

### Caps

The caps are public constants in `protocol` (`crates/protocol/src/web.rs`, re-exported from the
crate root), each documented on the field it limits. Producers build the payloads with their
constructors, which apply the caps: `WebSearchRecorded::new`, `WebResult::new` and
`PageVisited::new`. A longer value is cut, not refused, at a character boundary (a `char`, a
Unicode scalar value), so a multibyte character is never split. The constructors cut the URL after
the URL rule has run, so a secret past the cut is redacted before the cut removes it. The caps are
enforced on what is written; a log entry read back from an older writer is not re-capped.

| Field | Cap | Constant |
|---|---|---|
| `WebSearchRecorded.query` | 1,000 characters | `MAX_SEARCH_QUERY_CHARS` |
| `WebSearchRecorded.results` | 50 entries | `MAX_SEARCH_RESULTS` |
| `WebResult.title`, `PageVisited.title` | 300 characters | `MAX_WEB_TITLE_CHARS` |
| `WebResult.url`, `PageVisited.url` | 4,096 characters | `MAX_URL_CHARS` |

### The URL rule

Every URL the log keeps (`PageVisited.url`, each `WebResult.url`) goes through one pure function,
`logged_url`, which the constructors call. For an absolute `http` or `https` URL it:

1. keeps the query string, and drops the fragment (from the first `#`) and any userinfo
   (`user:pass@`);
2. replaces with `REDACTED` the value of each query parameter whose name matches, case-insensitively
   and after percent-decoding the name, one of: `token`, `access_token`, `id_token`, `key`, `apikey`,
   `api_key`, `sig`, `signature`, `auth`, `password`, `pass`, `session`, `sid`, `code`, `state`,
   `X-Amz-*`, `X-Goog-*`, `Expires`, `AWSAccessKeyId`, `Policy`, `Key-Pair-Id`. A trailing `*` means
   a prefix match, so `X-Amz-*` matches every name that begins `x-amz-`, and no other name matches
   by prefix;
3. replaces with `REDACTED` the value of any parameter that is 32 or more characters long and made
   only of hex digits, or only of base64url characters (`A-Z a-z 0-9 - _`), with up to two `=`
   padding characters at its end;
4. leaves everything else byte for byte: the scheme, host, port, path, each other parameter's name,
   its percent-encoding and the parameters' order. A parameter with no `=` has no value, so it stays,
   unless the whole of it is a long hex or base64url value, which becomes `REDACTED`.

A string that is not an absolute `http` or `https` URL is kept with only its fragment dropped.

`protocol` has no I/O and a fixed dependency list (its row in ARCHITECTURE.md), and the `url` crate
is not on it, so `logged_url` parses by hand: the scheme, `//authority`, the path, `?query`, and
`#fragment`. The userinfo ends at the last `@` of the authority, so an `@` in the path does not count.

Why the query stays: court and government sites put a document's identity in the query string
(`judgment.php?diary_no=12345&id=67890`), and a log that dropped it would not say which judgment
the agent read. Why credentials and tokens go: a signed link's query carries a live signature and
credential that work for anyone who reads the log, and a userinfo or a token in a URL is a password
in plain text. The name list is the usual set of names that carry a credential or a signature; the
value rule catches a token under a name nobody listed, such as `ref`.

### The view

The fold files each record under the tool call it belongs to. `ToolCallEntry.web` holds a
`ToolCallWeb`, boxed, and is left out of the JSON when the call has no web records:

- `search`, a `ToolCallSearch` (`query`, and `results`: the event's own `WebResult`s, with the
  event's caps and nothing added). A `WebSearchRecorded` whose `tool_call` is the call's sets it;
  a later one for the same call replaces it.
- `pages`, a list of `ToolCallPage` (`url`, `title` when there is one, and `via`). Each
  `PageVisited` whose `tool_call` is the call's appends one, in the log's order. A page opened
  twice is listed twice, and the UI shows each URL once.
- At most `MAX_PAGES_PER_CALL` = 50 pages per call. The reducer ignores the 51st and every later
  one, and the 50 it keeps are the first 50 the log holds.
- A record whose `tool_call` matches no tool call of the thread is ignored: it changes nothing and
  is no anomaly.

A change to a call's web sends the call's entry as an `Upsert`, as any change to a tool call does.

## Deviations

- **`agent` is `AgentId`, not `AgentKind`.** The S26 brief named `agent: Option<AgentKind>`. The code
  uses `agent: AgentId`, the thread's agent registry id. Reason: [0002](0002-event-schema.md)
  requires every enum reachable from a logged type to be open. `AgentKind` is closed, and opening it
  would ripple through the `limits` crate, where it is a map key. `AgentKind::of` stays the one place
  an id becomes a kind, so a reader that wants the kind calls it.
- **The payloads carry no `thread` and no `at`.** The S26 brief named both. The envelope's `stream`
  and `ts` already hold them, and the log is the one place they are read from.

## The matches (S26)

The host writes a record when a tool call completes, from the last `rawInput`, `rawOutput`, `content`
and title it has for the call (`crates/host/src/sources/`). A call that starts completed is read at
once. A call that fails or is cancelled, one still open when the turn ends, and any update the host
drops as late or replayed (`actor/turn/updates.rs`) write nothing. The match goes by the call's
shape, not by the agent's id, and a record names the thread's agent by its registry id. The field
names below are the ones the adapters sent in the fixtures in `crates/host/tests/scenarios/`, written
from claude-agent-acp 0.89.1 and codex-acp 2.2.2 source; check 3 (recorded fixtures) is not yet run.

Claude Code (`sources/claude.rs`):

- **Which tool**: the ACP `name` (Clauseo's `ToolCall.tool_name`): `WebSearch` is a search,
  `WebFetch` a fetch. Any other name is no web tool. With no `name`, a call of kind `fetch` is a
  search when its title starts `Search "` and its `rawInput` has `query`, and a fetch when its title
  starts `Fetch ` and its `rawInput` has `url`.
- **Search**: the query is `rawInput.query`. The results are the `title` and `url` of each `Links:`
  array in `rawOutput` (the string's lines that start `Links: `), in order. When no array parses, they
  are the `Title (url)` lines of `content`, split at the last ` (`. An empty array is a parse with no
  results.
- **Fetch**: the page is `rawInput.url`, with no title, `via` `WebFetch`.

Codex (`sources/codex.rs`):

- **Which call**: kind `search`, no `name`, and a `rawInput` that has a `query` key. An MCP server's
  own search tool has a `name`, so it is no Codex search.
- **Query**: `rawInput.query`, else `action.query`, else `action.queries` joined with ", " (the first
  that is not empty). With none, no record.
- **Action**: `rawInput.action`, which the AIR shape and the standard shape both carry. `null`, or
  `type` `search`: a `WebSearchRecorded` with no results. `openPage` or `findInPage`: a
  `PageVisited` with `action.url`, `via` `CodexOpenPage`, no title. `other`, or a type not known: nothing,
  with a warning in the host log.

Decided here (not in the brief):

- **Only web URLs are kept.** A result or page URL must be `http` or `https` (the browser's rule,
  `sources/browser.rs` `web_url`). A result URL over 2,000 characters is dropped, before the URL rule.
- **Results**: the first 50 that remain, with the URL rule applied and repeats (by the logged URL)
  dropped. A title has its control and bidi characters removed; an empty title is the URL.
- **Queries and pages must be non-empty.** A search with no query, or a page with no web URL, is no
  record.
- **Unfinished calls are kept per thread**, up to 64, as the browser's opens are. Past that the list
  is cleared.
- **One record per tool call.** A repeat fetch of the same page is a second record.

## Consequences

The log now holds what the agent searched and opened, with the tool call and turn that did it, so a
later view can show sources from the log alone. The events are additive: an older reader sees them
as unknown kinds, and a newer one reads them with no migration. What a later change must respect:
the fields are now part of the log, so a rename or a removed field needs its own record under 0002,
and a new `PageVia` value needs its row and its corpus entry. The view carries the sources from this
PR; until the sources list lands, nothing shows them to the lawyer.

The PR that folds these events into reducer state must bump `REDUCER_SCHEMA_VERSION`. That PR is
this one (see "The view"): `ToolCallEntry.web` is a state field, so an older snapshot lacks the
sources a log holds, and the bump makes each such snapshot rebuild from the events. The number goes
from 7 to 8: 7 is the thread triage's (#294), which merged first.

A later change to the caps or to the URL rule changes what a new entry holds, and not what an old
one holds: an entry is never rewritten. It needs a record under 0002, and its corpus entry, if any,
keeps the value it was written with.

## Links

- [0002: Event schema](0002-event-schema.md): the rules for additive kinds, open enums and the
  corpus.

## Status

proposed

## Date

2026-11-02
