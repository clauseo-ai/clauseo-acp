# 0050: The case-law connector in the host

## Context

The case-law connector is a paid service. The connectors crate (0034) holds the lawyer's
credential: an OAuth sign-in or a firm's API key, kept in the OS keychain. R24 puts that
connector in front of the agents (Claude Code and Codex) without the credential ever reaching
them. The agents run sandboxed or not at all (0038, 0049), they share their homes across matters
(R20 P4a: `<data>/agents/<id>/home`), and their output is shown in the main window. So:

- the agent must reach the connector through something it cannot read the credential from;
- a prompt-injected agent must not be able to spend the lawyer's money without limit;
- the window that renders agent output must not be able to add keys or buy credits;
- the case-law results should show as sources, like web searches (0041).

This record states the design as built in six pull requests: #470 (PR1, protocol and settings),
#466 (PR2, `clauseo-mcp connector`), #481 (PR3, the relay core), #496 (PR4, launch wiring), #505
(PR5, state, commands and desktop) and this PR (PR6, sources, the hint and the pay link). The brief
is the private `tasks/stage-e/R24-connectors-in-host.md`.

## Decision

### The shape

- Each agent launch whose profile offers a `case-law` entry gets an MCP server,
  `clauseo-mcp connector case-law` (`crates/mcp/src/connector.rs`, #466). It passes each
  JSON-RPC line unchanged over loopback TCP to a relay in the host, and back.
- The relay (`crates/host/src/connectors.rs`, `connectors/relay/**`, #481) checks the hello,
  holds the input limits and the spend caps, and forwards each line to the connector through
  `clauseo-connectors` with the lawyer's credential. The credential never leaves the host.
- One listener per launch, on `sandbox::ports::bind_loopback` (D4): the Windows range with
  `SO_EXCLUSIVEADDRUSE`, retired ports on Unix.

### The hello

The first line on a connection is `{"clauseo_relay":1,"token":"<64 hex>","connector":"<name>"}`
(`connectors/relay/handshake.rs`). The host checks it as strictly as `clauseo-mcp` checks the
answer:

- exactly these three keys, each once (`deny_unknown_fields` on borrowed strings, so a field
  with an escape does not parse);
- version 1; another number gets `unsupported_version`;
- a token of 64 lowercase hex characters;
- a connector name of 1 to 32 lowercase letters, digits or hyphens, not starting with a hyphen,
  which must be this relay's connector (`wrong_connector`);
- the token compared in constant time against the SHA-256 the listener keeps
  (`relay.rs`, `token_matches`), so another listener's token is `bad_token`, and a revoked one
  is `closed`.

A hello that fails the shape closes the connection unanswered. One that is well formed is
answered `ok`, or refused with a code and closed. Limits (`connectors/limits.rs`): 1 KB per hello
(`MAX_HELLO_BYTES`), 5 s from the accept to its newline on the host clock (`HANDSHAKE_TIMEOUT`),
8 pending handshakes per listener (`MAX_PENDING_HANDSHAKES`), 4 connections past the hello per
token (`MAX_CONNECTIONS_PER_TOKEN`), one hello per connection. The hello is never logged, echoed
or kept: it holds the token.

### The token file

- The relay is bound and its token minted at launch prep, on the prep thread, beside the browser
  claims (D3; `actor/turn/launch_checks/relay.rs`). The token comes from the connectors crate's
  `new_relay_token` and is held as a `Secret`.
- It is written to `relay-token` in the session's run folder, `<data>/run/<session id>/`
  (`TOKEN_FILE`), through `write::replace_private_file`. On Unix that is mode 0600 in the 0700
  run folder. On Windows the file inherits the run folder's ACL (D8 as revised, below).
- The path is a `RunFolders` field, `relay_token`, documented "R17 passes this to
  `PolicyBuilder::read_only_in_state`" (`actor/turn/launch_checks.rs`).
- The token is never in the entry's arguments or variables: the entry carries
  `CLAUSEO_RELAY=127.0.0.1:{relay_port}` and `CLAUSEO_RELAY_TOKEN_FILE={relay_token_file}`
  (`profile/src/servers.rs`, `case_law`). `clauseo-mcp` reads the file itself.
- The agent's stderr redactor hides it (`LaunchSpec::with_redacted` in `process`, #496).
- Claude's data-folder deny list holds `run/*/relay-token` (`agent/data_fence.rs`), so a token
  read through Claude's own tools never lands in a transcript.
- When the session ends the token is revoked first (the relay answers `closed` from then on),
  then the run folder goes, and the relay stops when the last `RunFolders` clone drops in
  `discard`, after the agent's tree was killed. Nothing is accepted after that.

### The caps and the day count

`connectors/limits.rs`, starting values, not yet measured:

- **Turn:** at most 60 paid `tools/call`s per turn (`MAX_PAID_CALLS_PER_TURN`). A call while no
  turn is open counts against a between-turns bucket with the same caps, which resets only when
  the next turn starts.
- **Recharge:** at most 5 `recharge` calls per turn or between-turns bucket
  (`MAX_RECHARGE_CALLS_PER_TURN`).
- **Day:** at most 600 paid calls per local day for the install, across sessions
  (`MAX_PAID_CALLS_PER_DAY`), kept in `<data>/connectors/day.json` so a restart does not reset
  it. A count starts again on a later day, or on one more than a day earlier.
- What is paid: any tool but `account` (free) and `recharge` (its own cap), unknown names and
  other spellings included (`Spend::of`).
- The local day comes from a `LocalDay` seam (D5): desktop fills it from the OS
  (`OsLocalDay`), tests from a fixed offset. A paid call is counted in memory under the day lock,
  then waits until `day.json` holds that count or a newer one before it goes out; the folder is
  fsynced after the rename on Unix (#496).

### The spend path

On the parsed and re-serialised value (`connectors/relay/forward.rs`):

- the method must be exactly `tools/call` to count;
- a `tools/call` with no `id` is refused;
- a non-string `params.name` is refused;
- `protocolVersion` and `Mcp-Session-Id` are checked before they go back as headers;
- `initialize` and `server/discover` get `capabilities: {}`;
- a call is counted only after the credential and session checks.

**Refunds.** A counted call is given back in two cases only: the session was already ending, so
nothing went out; or the connector refused the credential (a 401 after S28's refresh and retry,
or an auth error) and so surely did not charge (`uncharged`, `refund`). A network failure or a
timeout is not refunded: it may have reached the connector after the charge. A refund reaches
only the bucket it was taken from.

### Raw replies

S28 hands back the connector's reply raw, up to 8 MB, and a session may have 16 in flight. The
review of #481 found that parsing each reply to a `serde_json::Value` (about 32 bytes per element
of a dense array) could reach gigabytes. So the reply stays raw JSON (`McpBody` holds a
`Box<RawValue>`), and the relay reads it through borrowed structs
(`connectors/relay/forward/reply.rs`): the top-level `id`, `method`, `result` and `error` as raw
slices, a small `id`, `initialize`'s `protocolVersion`, and a `tools/call`'s `isError` with its
`structuredContent` only when that is true, up to 64 KB. The answer's own bytes are the line. One
8 MB dense reply peaks at 8 MiB, not 432 (#496's `relay_memory` test). The relay's own 4 MB reply
check (`MAX_REPLY_BYTES`) stands in front, since S28's limit is 8 MB.

### The state and the commands

- `HostCommand::Connector(ConnectorCommand)` (`protocol/src/command/connectors.rs`): `check`,
  `add`, `add_api_key`, `add_credits`, `sign_out`, `remove`, `refresh_balance`,
  `dismiss_case_law_hint`, and from this PR `open_pay_link`. None stores a receipt (D2); a reused
  `command_id` of a connector command is a conflict, not a join, since keys hash alike as
  `"[redacted]"`. `RedactedString` encodes and prints as `"[redacted]"` and is zeroed on drop.
- The commands run off the actor (`actor/connectors.rs`). With `tools.connectors` off, all but
  `sign_out` and `remove` are refused.
- **Credential generation.** Sign-out, remove and the switch going off bump it: a key or sign-in
  still in flight then stores nothing, and a read that started before is not applied.
- The account is read at most once a minute for a paid result, `check` and `refresh_balance`.
- **Money as integers.** Amounts reach the app in paise (`price_paise`, `PackView.paise`,
  `Balance.paise`), so the app formats them by its own money rule (`connectors/state.rs`). A
  figure that does not read as rupees with at most two paise digits is left out, never guessed.
  Every amount the lawyer sees includes GST (what they see is what they pay); the GST
  breakdown is on the connector's receipt, not in the app. The account's spend and balance are
  credits, priced by the host at the price list's `credit_inr` plus `gst_rate` (integer
  arithmetic, rounded half-up to the paise once per amount; none at all if either rate does not
  read). `Balance.paise` was the account's net `balance_inr` before this; older logs hold net.
- The pay link is held only as a `PayUrl`, which only the connector's own `/pay/` links parse
  to (D1: `ConnectorView.pay_url` in `ConnectorsChanged`, no new `CommandOutcome`).
- **The wire-gaps follow-up.** Four additive `Option` fields, left out when `None`, that this
  host always fills where they apply (`ConnectorsChanged` is not folded, so no schema version
  changes): `ConnectorStatus::Error.kind` (`sign_in_failed`, or `persist_failed` when the
  connectors crate holds refreshed tokens in memory only, shown once when that turns true while
  signed in); `SignedIn.method` (`account` for the browser sign-in, `api_key` for a pasted key);
  `Balance.as_of` (the host clock's wall time when the account read was applied); and
  `ConnectorView.last_recharge` (`paid`, `expired`, `cancelled`), set when the watched
  recharge's pay link clears. A watch that reaches its deadline counts as `expired`; one that
  fails otherwise sets nothing. The outcome lasts until the next recharge is watched, with no
  timer, and goes with a sign-out, a remove, the switch going off or a credential that ended.
- **Proxy.** Desktop builds the connector's route as `ProxyChoice::Resolve`, asking
  `clauseo_sandbox::upstream_for` before each request, so the connector goes the way the host's
  own HTTPS does (`desktop/src/connectors.rs`). A refused proxy setting fails each request; the
  error is not logged, since it can quote the setting.

### The window split

Like `SetSkillPack`, by window (`desktop/src/bridge/admit.rs`):

- `main` renders agent output, so it sends only `dismiss_case_law_hint` and `refresh_balance`.
- `check`, `add`, `add_api_key`, `add_credits`, `sign_out`, `remove` and `open_pay_link` are
  settings-window only. The settings window may not send `dismiss_case_law_hint`.
- The room's hint and out-of-credit banner open Settings at the Connectors section, through
  `open_settings` with a new optional `section: "connectors"` (this PR). The enum is closed,
  as the app asked: an unknown value is refused as the arguments decode, so a page and a build
  that disagree fail loudly rather than open the wrong place, and a new section ships with the
  page that shows it. A new window opens at `settings.html#connectors`; an open one gets
  `location.hash` set to that fixed string (never the caller's text). The page will follow it
  on `hashchange` once U15 builds the section; no listener exists yet.

### The pay link opens with a confirm

`ConnectorCommand::OpenPayLink { id }` (this PR, settings window only) opens one link: the
https `PayUrl` that `add_credits` got from the recharge API, which the connectors crate built
and checked against our host, held in the connector state (`CaseLaw::pay_link`). Before it
opens, the host parses it again as a production pay link (https, host exactly
`data.clauseo.chat`, no port or user info, path `/pay/<id>`); anything else is refused
(`command.invalid`), and so is the command when there is no such link.

- It is never a URL from relay output, a tool result or the agent. Out of credit's link reached
  the host in a tool result the relay passed on, so it is shown in `OutOfCredit` but never
  opened by this command; the lawyer adds credits in Settings, which makes a fresh recharge.
  A host test makes a search (whose results hold links) and an out-of-credit call through the
  relay and checks that `open_pay_link` then refuses and opens nothing.
- It opens through `ConnectorOpener::open_confirmed`, which desktop runs as H11's
  `open_external` (0026): the native confirm naming where it goes, its one-at-a-time gate and
  its pause after a decline. It never goes through `open_url_unconfirmed`, which stays the
  sign-in page's alone. No URL comes from the window: the command carries the connector's id.
- The confirm waits on the lawyer, so it runs on its own thread and the command is answered when
  it ends (declined: `transport.unauthorized`).

### The pre-allow (D7)

Claude gets `mcp__case-law` in `allowedTools` only when the host sends our entry
(`agent/loads/case_law.rs`, `CLAUDE_ALLOW`); otherwise any hook rule for that server is dropped.
`case-law` is a reserved server name (`settings/src/leaf.rs`, `RESERVED_MCP_SERVERS`): a firm's or
the lawyer's `mcp.servers.case-law` is dropped with a visible warning ("the server name case-law
is reserved for Clauseo's case-law connector"). No server name may hold `__` or end in `_`, since
Claude splits `mcp__<server>__<tool>` at `__`. So a firm's server can never inherit the
pre-allow. Forced-ask sessions drop `allowedTools` entirely, so case law asks there.

### Case-law sources (this PR)

- When a `search_law` or `read_document` call answers, the relay emits
  `RelayEvent::CaseLawFound` with the tool, a join key and the titles and links it parsed from
  the reply. The key is the SHA-256 of the tool's name and the canonical JSON (sorted keys) of
  its arguments, so the host keeps no argument text. `CaseLawFound`'s `Debug` prints no title
  or link: they can echo the lawyer's search words.
- Only a result that is no error is read (`isError` true reads nothing), and only after the
  relay's 4 MB check; at most 50 entries of a list are read, the rest skipped unparsed.
- The actor keeps these as per-thread join entries for 5 minutes (at most 64 per thread), each
  taken once.
- When the matching ACP tool call completes, it takes the entry: Claude's tool is
  `mcp__case-law__search_law` or `mcp__case-law__read_document`; Codex titles it
  `mcp.case-law.<tool>` with the arguments under `arguments` (codex-acp 2.2.2,
  `sources/browser.rs`). The host logs `WebSearchRecorded { source: CaseLaw, tool }` for a
  search and one `PageVisited { via: CaseLaw }` per document read (0041's events; the fields are
  additive under 0002, #470).
- With no entry, the search is recorded with its query and no results, and a read records
  nothing. A call whose result is an error records nothing; `citator`, `check_citations`,
  `account` and `recharge` record nothing. A call records only in the turn it started in.
- A replay adds none (`actor/turn/updates.rs` already skips replays), even with an entry
  waiting; a host test checks it.
- The paths read from the reply, `structuredContent.results[]` and `documents[]`, are not yet
  checked against the real connector.

### The hint (this PR)

At a turn's end, over that turn's `WebSearchRecorded { source: Web }` records, the host may
publish `CaseLawHint { matter, thread, turn }` once per matter (`connectors/hint.rs`,
`actor/connectors/hint.rs`).

- The heuristic is hand-written (the host has no regex crate) and runs on each query and result
  URL as each record is logged: the citation marks `SCC`, `AIR`, `SCR`, `INSC` as whole
  words, case-sensitive; a neutral citation `YYYY:COURT:N`; ` v. `, ` vs `, ` vs. `; "supreme
  court", "high court", "nclat", "tribunal", "judgment", "judgement", "precedent"; and a result
  host on S27's seed list or under one, or `indiankanoon.org`. A table test holds 22 queries
  that fire and 21 that must not.
- At the turn's end the host checks, in order: `tools.connectors` on; connectors wired; the
  connector known to be `NotAdded`; the matter not in `<data>/connectors/hints.json`. Only
  then does it ask liveness, through a `check` job with no account read, so a lawyer who never
  searches case law sends nothing to Clauseo's server. Any liveness answer settles every matter
  waiting; if live and the checks still pass, the hint is published and the matter marked
  `shown`.
- `DismissCaseLawHint` marks the matter `dismissed`. Either mark keeps the hint away for
  good. The file is `{"v":1,"matters":{"<id>":"shown"|"dismissed"}}`, written whole and
  atomically on the actor (rare: once per matter); a damaged file counts as no marks, so the
  worst it does is offer the hint once more.
- The event is an offer, not part of the record: never logged.

### The skill line

R22's `legal-research` skill says, when case-law tools are available in the session, to use
them first for Indian judgments and citations (version 3 in `versions.lock`; #496 made it
conditional as version 2).

### The paragraph

The composed instructions hold a `{connectors}` token (D6) that the host fills with the
connectors paragraph (66 words, in profile assets) in sessions with our entry. A session without
it gets `connectors-none.txt` instead ("You have no case-law database in this app"), so the two
never meet (`profile/src/instructions.rs`, `connectors_part`; `actor/turn/profile_loads.rs`).

### Decisions as applied

- **D1.** The pay link reaches the app in `ConnectorView.pay_url`; `add_credits` replies `Done`.
- **D2.** Connector commands skip receipts; `RedactedString` serialises `"[redacted]"`. A test
  finds the key in no log line, `Debug` output, event, snapshot, data folder, agent home or
  policy file (#505).
- **D3.** The relay and token are made at launch prep and ride on `RunFolders`; the token is
  revoked when the run ends, not only its file removed.
- **D4.** `sandbox::ports::bind_loopback` for the port.
- **D5.** A `LocalDay` seam on the host's config; desktop fills it from the OS.
- **D6.** A `{connectors}` token in the composed instructions; the words stay in profile assets.
- **D7.** The pre-allow only with our entry, and a reserved name.
- **D8, revised with stage C (2026-11-05).** No protected DACL on Windows: the token file
  inherits the run folder's ACL. Under tier B the matter's pool user has Modify on
  `run/<id>` and other pool users are denied (0049); outside tier B it is the data folder's
  profile ACL. Not built yet (see "Left for others"): R17 will mark the file read-only in the
  sandbox policy (tier B deny-write, Seatbelt `deny file-write*`) and will put the relay port
  in `Loopback::Ports`; S15 will record the WFP row.

### The credential exception

ARCHITECTURE's invariant "Clauseo never reads an agent's credentials" is about the agents' own
logins. The connector's credential belongs to Clauseo, which holds it for its own connector
(0034). The relay reads it, in the host only, to forward calls; it never goes to an agent, a
window, a log or a receipt.

### Logging

Logs name the session, the method and the reason, never a line's content, an argument, a URL
query or a token. `Secret` prints as `Secret(..)`.

## Consequences

**Tier C.** Where the agent runs unsandboxed, or under any sandbox that lets another process of
the same user read the run folder, that process can read the token file and spend credits up to
the caps: 60 paid calls per turn (or between turns), 600 per local day, and 5 recharges per turn.
The caps, not the token, bound the loss there.

Accepted residuals:

- **Handshake starvation.** Eight idle loopback sockets hold every pending slot until their 5 s
  run out, again and again. Denial of service only; no spend.
- **Clock jump.** A lawyer who moves the clock forward a day resets the day cap.
- **Corrupt `day.json`.** A file that does not read, or names no day, counts from 0, with a
  warning. A same-user process could do this on purpose (tier C).
- **The managed-MCP window.** For Codex the instructions are fixed right before the launch, and
  `session_loads` reads `managed-mcp.json` once more after the agent answers `initialize`. A
  file that appears in between drops the entry (`ManagedByFirm`, the relay ended) while the
  paragraph stays: wrong words, but no case law reaches the agent
  (`actor/turn/profile_loads.rs`).
- **Folders made after the launch.** The launch's data-folder deny rules enumerate what exists
  at launch prep; run folders and matters' notes made after the launch are in no rule
  (`RunFolders.data_deny`, `actor/turn/launch_checks.rs`). This residual predates R24.

What a later change must respect:

- The token never goes in an argument, a variable, a log, an agent home or an event.
- Every count is on the re-serialised value; a new free tool is a change to `Spend::of`.
- A reply is never parsed whole.
- Only the settings window spends or adds a credential; a new connector command picks its window
  in `admit.rs`.
- `case-law` stays reserved while the pre-allow exists.
- `budgets.toml` holds the `connector_relay` bench's rows.

Left for others: R17's policy entries and port (stage C), S15's WFP permit, the Settings section,
hint and banner screens (U15), and the live connector's result shapes for the sources join.

## Links

- [0026: Open and reveal](0026-open-and-reveal.md): `open_external` and its confirm.
- [0028: The settings window](0028-settings-window.md): `open_settings`.
- [0030: Launch profile](0030-launch-profile.md): the profile's servers and instructions.
- [0034: The connectors crate](0034-connectors-crate.md): the credential, `Secret`, `PayUrl`.
- [0038: Sandbox](0038-sandbox.md) and [0049](0049-windows-sandbox-tier-b.md): the tiers.
- [0041: Web searches and sources](0041-web-search-and-sources.md): the events reused here.

## Status

proposed

## Date

2026-11-06
