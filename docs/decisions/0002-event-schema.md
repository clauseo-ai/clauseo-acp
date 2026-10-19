# 0002: Event schema

## Context

Every floor 2 crate builds on the types in `crates/protocol`: the envelope the log stores, the domain
events inside it, the view patches and host events a subscriber receives, and the rules that let an
older reader keep working on a newer log (synthesis §2.5, §2.6; doc 04 §3.2; doc 11 §7). Once the log
holds a lawyer's work, its JSON form cannot change by accident. This record freezes that form.

Matter-level events (version history spanning every thread in a matter) had no home in an envelope
keyed by `thread_id`. The envelope therefore names a stream, a thread's or a matter's, and `seq`,
cursors and subscriptions count per stream (QR5.1, answer A).

## Decision

### Envelope

```rust
pub struct Envelope {
    pub schema_version: u32,           // 1 at the freeze
    pub event_id: EventId,             // UUIDv7, minted by the host
    pub stream: StreamId,              // "thread:<uuid>" or "matter:<uuid>"
    pub seq: Seq,                      // gapless per stream, starts at 1, assigned at commit
    pub ts: Timestamp,                 // ms since the Unix epoch, UTC; display only
    pub command_id: Option<CommandId>, // the command that caused it, if any
    pub event: DomainEvent,            // {"<kind>": {...payload}}
}
```

```json
{"schema_version":1,"event_id":"01920000-0000-7000-8000-000000000001",
 "stream":"thread:01920000-0000-7000-8000-000000000002","seq":42,"ts":1760000000000,
 "event":{"text_appended":{"turn":"01920000-0000-7000-8000-000000000003",
  "message":"01920000-0000-7000-8000-000000000004","kind":"answer","text":"Done."}}}
```

- Order by `seq`, never by `ts`. Events are append-only: a correction is a new event, and nothing
  rewrites one. Old events are upcast on read, never rewritten.
- `DomainEvent::kind()` returns the event's key (`"text_appended"`), or the stored kind of an
  `Unknown`; `store` fills its `kind` column from it. `DomainEvent::KINDS` lists every known kind.
- Ids are newtypes over a UUID in its 36-character text. Clauseo mints every id as UUIDv7 except
  `CommandId`, which the client generates; `protocol` itself never mints an id or reads a clock. ACP
  session, tool-call and message ids are attributes, never keys. No path is ever an id.
- `Seq`, `Cursor`, `Timestamp` and every other 64-bit number stay below 2^53, so TypeScript reads
  them as a `number`.

### Kinds

Stream: T is the thread's stream, M the matter's. "Emitted by" names the component that decides the
event; the host commits every logged one. `?` marks an optional field, left out when empty.

| Kind | Payload | Emitted by | Logged | Stream |
|---|---|---|---|---|
| `thread_created` | matter, agent, title? | host | yes | T |
| `thread_renamed` | title, by (lawyer or agent) | host, acp | yes | T |
| `thread_archived` | nothing (`{}`) | host | yes | T |
| `agent_session_attached` | session, agent_version?, capabilities | acp | yes | T |
| `agent_session_detached` | reason (`DetachReason`) | acp, host | yes | T |
| `user_message` | turn, message, acp_message_id?, content, delivery? | host; acp on replay | yes | T |
| `turn_queued` | turn, behind, behind_turn? | host | yes | T |
| `queued_prompt_edited` | turn, message, content | host | yes | T |
| `steer_resolved` | message, outcome (`SteerOutcome`) | host | yes | T |
| `turn_started` | turn, checkpoint_before | host | yes | T |
| `text_appended` | turn, message, acp_message_id?, kind (answer or thought), text | acp | yes | T |
| `content_appended` | turn, message, acp_message_id?, kind, block (never text) | acp | yes | T |
| `tool_call_started` | turn, tool_call, acp_id, title, kind, tool_name?, status, locations, raw_input?, raw_output?, content | acp | yes | T |
| `tool_call_updated` | tool_call, patch (every field optional; lists replace wholesale) | acp; host on cancel | yes | T |
| `permission_requested` | request, tool_call?, options, deliverable | acp | yes | T |
| `permission_resolved` | request, choice, by | host | yes | T |
| `permission_expired` | request, reason | host | yes | T |
| `elicitation_requested` | elicitation, request (form schema or URL) | acp | yes | T |
| `elicitation_resolved` | elicitation, outcome, by (never the values) | host | yes | T |
| `elicitation_expired` | elicitation, reason | host | yes | T |
| `plan_replaced` | entries | acp | yes | T |
| `mode_changed` | mode_id | acp | yes | T |
| `config_options_replaced` | options | acp | yes | T |
| `commands_available` | commands | acp | yes | T |
| `usage_updated` | context_used?, context_size?, cost? | acp | yes | T |
| `notice` | level, text | acp, host | yes | T |
| `compaction_changed` | status, summary? | acp | yes | T |
| `files_changed` | turn, files, skipped | matters, through the host | yes | T |
| `turn_failed` | turn, code, detail? (never shown) | host, acp | yes | T |
| `turn_stopped_at_limit` | thread, turn, window?, resets_at?, reason | limits, through the host | yes | T |
| `turn_ended` | turn, stop, usage?, checkpoint_after? | host | yes | T |
| `step_undone` | turn, checkpoint, restored, kept | host, matters | yes | T |
| `matter_created` | name, kind, import? | host | yes | M |
| `checkpoint_recorded` | checkpoint, kind, thread?, turn? | matters, through the host | yes | M |
| `outside_edits` | checkpoint, files | matters, through the host | yes | M |
| `version_restored` | from, checkpoint, scope | host, matters | yes | M |
| `patches` (`HostEvent`) | `PatchBatch`: stream, epoch, patches | host | no | the subscribed stream |
| `events` (`HostEvent`) | envelopes, replayed then live | host | no | the subscribed stream |
| `caught_up` (`HostEvent`) | stream, last_seq; once per `SubscribeEvents`, between replay and live | host | no | the subscribed stream |
| `account_limits_changed` (`HostEvent`) | `AccountLimits` | limits, through the host | no | none |

- Exactly one `turn_ended` per turn, including errors, cancels and a queued turn cancelled before it
  started. `turn_failed` and `turn_stopped_at_limit` add detail and never replace it;
  `turn_stopped_at_limit` may follow its turn's `turn_ended`, at most once. `files_changed`, when a
  turn changed files, comes before `turn_ended`.
- `user_message` comes before `turn_queued` or `turn_started`, so a queued turn shows its prompt.
- `DomainEvent` is what `store` accepts; `HostEvent` is what a subscriber receives and is never logged.
  Limit ticks stay out of the log; `store` keeps the last `AccountLimits` per account apart.

### View patches and entries

- `ViewPatch { cursor, body }`; `ViewPatchBody` is `Upsert { entry }`, `Remove { entry_id }`,
  `AppendText { message_id, from_len, text }` or `Reset { view_snapshot }`. A `ViewSnapshot` holds
  the header, about 200 recent entries and `has_older`. A `PatchBatch` carries the
  stream and the `ViewEpoch` once. The webview applies only the next cursor, and `AppendText` only
  when its text length equals `from_len`, counted in UTF-16 code units (`utf16_len`).
- Cursors live in host memory: text reaches the screen before its event is durable, so a cursor is
  never a `seq`. The host mints a `ViewEpoch` each time it loads a stream.
- `Subscribe { stream, after: Option<ViewPosition { epoch, cursor }> }` returns the tail after that
  position, or a `Reset` when `after` is missing, the epoch differs or the tail is gone. Older
  entries page in through `HostQuery::PageEntries`.
- `SubscribeEvents { stream, after_seq: Option<Seq> }` serves envelopes by `seq`, then `caught_up`,
  then live ones, with no gap or duplicate (`cli` and a later phone client use it).
- `Ack { stream, epoch, cursor }` carries the cursor the client applied.
- Entries carry no `id` field. Each entry starts with `order` (the `seq` of the event that created it;
  entries sort by it) and holds the domain id it is built from. A client keys an entry as
  `<variant key>:<id>`, the text form of `EntryId`:

| Entry | Key |
|---|---|
| `Header` | `header:<thread id>` |
| `UserMessage` | `user_message:<message id>` |
| `AssistantMessage` | `assistant_message:<message id>` |
| `AgentContent` | `agent_content:<event id>` |
| `ToolCall` | `tool_call:<tool call id>` |
| `Permission` | `permission:<permission request id>` |
| `Elicitation` | `elicitation:<elicitation id>` |
| `Step` | `step:<turn id>` |
| `LimitStop` | `limit_stop:<turn id>` |
| `Notice` | `notice:<event id>` |
| `Marker` | `marker:<event id>` |
| `MatterHeader` | `matter_header:<matter id>` |
| `Version` | `version:<checkpoint id>` |

### Serde conventions

- `rename_all = "snake_case"` for fields and variants, on the wire and on disk.
- Every enum with data, the envelope's `event` included, is externally tagged: the kind is the key of
  a one-key object (`{"text_appended": {...}}`, `{"inline": "<text>"}`). A unit variant is a plain
  string (`"pending"`). No `tag`, no `tag` with `content`: no internally or adjacently tagged enum.
  Measured on an Apple M6 Mac mini over 20,000 events: external tagging decodes 15 to 25% faster in
  Rust and parses 6 to 16% faster in the webview engines than the other shapes.
- `StreamId` and `EntryId` are one plain string, `<kind>:<uuid>`; the same text serves the wire, SQL
  and the command line.
- Every `Option` field is left out when `None`, never written as `null`, and a missing field reads as
  `None`. Events accept fields they do not know, so an older reader takes a newer optional field.
  Commands refuse unknown fields.
- Events decode in one pass: the kind is matched as a borrowed string and its payload decoded straight
  into its type. A known kind with a payload that does not decode is an error. The same holds in JSON
  and in MessagePack (the stored form).
- Large variants are boxed; boxing does not change the JSON.

### Enums that decode unknown values

A newer Clauseo may add a variant to these; an older reader decodes it to the catch-all instead of
failing. This version never produces one, except `ContentBlock::Unsupported` for an ACP block kind
`acp` does not know.

- `DomainEvent::Unknown { kind, payload }`: the payload is kept as `RawPayload` and written back under
  its own kind, byte for byte in the format it came from; it still advances `seq`.
- `ContentBlock::Unsupported { kind, raw_from, raw }`: the whole unknown block kept as compact JSON.
- `ErrorCode::Unknown`: the original code moves into `HostError.message`.
- Unit-only enums with a unit `Unknown`: `ToolKind`, `ToolCallStatus`, `PermissionKind`, `StopReason`,
  `NoticeLevel`, `Deliverable`, `CheckpointKind`, `ExpiryReason`, `RenameSource`, `Delivery`,
  `SkippedFileReason`. A command carrying `Delivery::Unknown` is refused with `command.invalid`.
- Enums with data, with `Unknown { kind }` (the payload is dropped in memory; the log keeps the
  original event): `DetachReason`, `LimitSource`, `SteerOutcome`.
- Every other enum is closed.

### Inline limit

`TextOrBlob` is `Inline(String)` or `Blob(BlobRef { hash, size_bytes, preview })`, with a BLAKE3 hash
and a preview of at most `PREVIEW_BYTES` (512). The inline parts of one view entry cost at most
`INLINE_LIMIT_BYTES` (4096): UTF-8 bytes after JSON escaping, without the quotes, summed over the
entry's `Inline` values, `Vec` items included. Fields are taken in declaration order: for a tool call
`raw_input`, `raw_output`, then `content` items; for a user message its `content` items; for agent
content its block; for a marker its `summary`. A text patch costs its text. `escaped_len` and
`TextOrBlob::inline_len` are the only way to count it; the host enforces it when it builds the event.
Message text stays a `String` and streams as `AppendText` patches under the same limit.

### Freeze rules

- Additive changes keep `schema_version`: a new kind, a new `Option` field that is left out when
  `None`, or a new variant in one of the enums that decode unknown values. Each still needs a decision
  record and Rohan's approval.
- Renaming, removing or retyping anything, or a new variant in a closed enum, bumps `schema_version`
  and adds an upcaster in `protocol`.
- Golden snapshots of one canonical example per kind, view patch, host event, command and content
  variant pin the JSON; a changed snapshot is a schema change.

### Dependency row

`ARCHITECTURE.md`'s `protocol` row becomes `exists`, and its "May depend on" grows from serde, uuid
and specta to serde, serde_json, uuid, compact_str and specta.

- `serde_json` 1.0.154, no features (QR5.2, answer a): it writes `Unsupported.raw` as compact JSON and
  backs the escaped-length tests; a JSON writer of our own would be one more place to get escaping
  wrong. sonic-rs and simd-json were measured on an Apple M6 Mac mini and rejected: sonic-rs encodes
  26 to 59% faster but pulls in 30 crates against 5 and has 250 `unsafe` sites against 16, and
  simd-json is slower on small inputs.
- `compact_str` 0.10.0, with `serde`: short strings (ids, titles, tool names, MIME types, option ids)
  need no heap block up to 24 bytes; cloning is 85% faster on an Apple M6 Mac mini. `protocol`
  re-exports `CompactString`.
- `specta` stays optional, behind a feature only the app turns on. "Never depends on" is unchanged:
  tauri, tokio, fs, process.

## Consequences

- `store`, `reducer`, `host`, `acp`, `cli` and the webview share one envelope, one kind table and one
  text form for streams and entries. A Versions timeline is a fold of the matter's stream like any
  other view.
- An older Clauseo reads a newer log: unknown kinds and variants decode to their catch-all variant and unknown
  optional fields are skipped, so additive changes need no migration.
- Any change to events, commands, view patches or error codes after this record needs a new decision
  record and Rohan's approval; a breaking one also needs an upcaster.
- Message text stays inline and unbounded per message; only agent payloads that land in one entry go
  to the blob store.

## Status

Proposed. Accepted once Rohan approves the pull request that adds this record; that approval is the
event schema freeze.

## Date

2026-10-19
