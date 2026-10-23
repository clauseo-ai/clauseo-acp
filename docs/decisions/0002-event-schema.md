# 0002: Event schema

## Context

Every crate that reads or writes the log or talks to a client (`store`, `reducer`, `host`,
`transport_ws`, `acp`, `cli`, the desktop app) builds on the types in `crates/protocol`: the envelope
the log stores, the domain events inside it, the view patches and host events a subscriber receives,
and the rules that let an older reader keep working on a newer log. Once the log holds a lawyer's
work, its JSON form cannot change by accident. This record freezes that form.

Matter-level events (version history spanning every thread in a matter) had no home in an envelope
keyed by `thread_id`. The envelope therefore names a stream, a thread's or a matter's, and `seq`,
cursors and subscriptions count per stream.

## Decision

### Envelope

```rust
pub struct Envelope {
    schema_version: u32,               // 1 at the freeze; private: only Envelope::new builds one
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

- An envelope's `schema_version` picks how its `event` is read. Serde writes it first, in JSON and
  in MessagePack named maps, so a written row's event is decoded in place, never buffered; an
  `event` key before it (sorted keys, as a `serde_json::Value`, a jsonb column or `jq -S` put
  them) is held as read, any key and as deep as the format nests (`serde_json`'s recursion limit,
  a MessagePack reader's `MAX_MSGPACK_DEPTH`), and decoded once the version is known by the same
  rules as in place: `MAX_RAW_DEPTH` and the map-key rule apply only where the event decodes to a
  `RawPayload`, each counted from that payload's own root. The one difference is `serde_json`'s
  limit: in place it skips a newer event or an unknown field without counting its depth, while
  the held copy stops at 128 levels with the envelope's map. The version picks how the event is
  read: at `SCHEMA_VERSION` as this version's `DomainEvent`; older through
  `protocol`'s one upcast hook, `upcast` in `envelope.rs`, which turns the old payload, read as
  `RawPayload`, into today's event (at version 1 nothing is older, so it refuses); newer not at
  all. A store decodes a row as `EnvelopeRead`, whose `Newer { schema_version }` holds only the
  version and skips the rest of the map. The store returns `store.newer_version` for `Newer` and
  `store.corrupt` for a decode error, in JSON and in MessagePack alike. A value, not an error,
  says "newer": plain `Deserialize` returns each format's own error type, so an error could only
  be told apart by its text, and `protocol` cannot offer a MessagePack decode function of its own
  (`rmp-serde` is not one of its dependencies). Decoded as a plain `Envelope`, a newer entry is
  an error.
- Order by `seq`, never by `ts`. Events are append-only: a correction is a new event, and nothing
  rewrites one. Old events are upcast on read, never rewritten; a decoded envelope's
  `schema_version` is always `SCHEMA_VERSION`, while its row keeps the version it was written
  with.
- `DomainEvent::kind()` returns the event's key (`"text_appended"`), or the stored kind of an
  `Unknown`; `store` fills its `kind` column from it. `DomainEvent::KINDS` lists every known kind.
- Ids are newtypes over a UUID in its 36-character text. Clauseo mints every id as UUIDv7 except
  `CommandId`, which the client generates; `protocol` itself never mints an id or reads a clock. ACP
  session, tool-call and message ids are attributes, never keys. No path is ever an id.
- `Seq`, `Cursor`, `Timestamp` and every other 64-bit number stay at most 2^53 - 1 in magnitude, so
  TypeScript reads them as a `number`: each is a checked type (counts and sizes are `Count`) that
  cannot hold a larger value and refuses one on decode. Floats are `Finite`: never NaN or an
  infinity.
- `MatterPath` is matter-relative with `/` separators. It refuses an empty, `.` or `..` segment, a
  leading `/`, `\`, every Unicode control character (`char::is_control`: C0, DEL and C1), a first
  segment of the form `X:` (a drive), a segment over 255 bytes and a path over 4096 bytes. It allows
  bidi marks and other format characters, since Hebrew and Arabic file names hold U+200F, so the UI
  shows every path bidi-isolated. It allows `:` elsewhere, since macOS file names may hold one. On
  Windows `matters` must refuse a path with a `:` (an alternate data stream), a part for which
  `is_windows_reserved_name` is true (the one list of device names, also used for a new matter
  folder's name) and a part ending in a dot or a space. Two paths are equal only when their bytes
  are: no case folding and no Unicode normalisation, although APFS and NTFS ignore case and APFS
  ignores normalisation (`café.docx` in NFC and in NFD are two paths). `matters` and the UI compare
  paths the same way, and the host matches an `UndoStep`'s paths against the step's `FilesChanged`
  exactly, so a path the file system would read as another file is refused.

### Kinds

Stream: T is the thread's stream, M the matter's. "Emitted by" names the component that decides the
event; the host commits every logged one. `?` marks an optional field, left out when empty. The
`kind_table_matches_record` test reads this table: the logged kinds, in order, are
`DomainEvent::KINDS`, and the Stream column matches `DomainEvent::is_matter_level` (`Some(true)`
for M, `Some(false)` for T), which comes from the stream each row of `event.rs`'s kind table names.
An unknown kind has no row and no answer (`None`): its envelope's `stream` decides.

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
| `turn_stopped_at_limit` | turn, window?, resets_at?, reason | limits, through the host | yes | T |
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
- A `tool_call_updated` patch leaves a field `None` when it did not change, so no patch takes a
  field back to absent: a call that has a `tool_name`, `raw_input` or `raw_output` keeps one. A
  new tool-call field touches `ToolCallStarted`, `ToolCallPatch` and the view's `ToolCallEntry`,
  plus the reducer's merge.
- `DomainEvent` is what `store` accepts; `HostEvent` is what a subscriber receives and is never logged.
  Limit ticks stay out of the log; `store` keeps the last `AccountLimits` per account apart.

### View patches and entries

- `ViewPatch { cursor, body }`; `ViewPatchBody` is `Upsert { entry }`, `Remove { entry_id }`,
  `AppendText { message_id, target, from_len, text }` or `Reset { view_window }`. A
  `ViewWindow` holds the header (a `StreamHeader`: a thread's `Header` or a matter's
  `MatterHeader`, no other entry kind), about 200 recent entries and `has_older`. A `PatchBatch`
  carries the stream and the `ViewEpoch` once. The webview applies only the next cursor, and
  `AppendText` only when the text it extends is `from_len` long, counted in UTF-16 code units
  (`utf16_len`).
- `AppendText`'s `target` is an `AppendTarget`: `AgentText` extends an agent text's `text`;
  `UserBlock(i)` extends the `Text` block at index `i` of a user message's `content`. Message ids
  are unique across both kinds.
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
  `<variant key>:<id>`, the text form of `EntryId`. `ViewEntry`, `EntryId` and the id each entry
  holds come from one table (`view_entries!` in `view.rs`), so the key and the id's kind are the
  same text:

| Entry | Key |
|---|---|
| `Header` | `header:<thread id>` |
| `UserMessage` | `user_message:<message id>` |
| `AgentText` | `agent_text:<message id>` |
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

The reducer writes a `Step` only for a turn that started (its `turn_started` is in the stream),
when that turn ends. A turn that ends without starting, a queued prompt removed or cancelled before
its turn, gets no step card, only its user message's `UserMessageStatus::Removed`: it ran nothing
and changed no file, so a card would have nothing to show or undo.

A `Step` entry has no `undo_possible` field. The UI derives it from the step alone, as
`checkpoint_after` exists and the step is not undone (`StepEntry::undo_possible` states the same
rule). Whether a turn runs is the header's `status` for this thread and the host's check at
`UndoStep` for the matter's other threads.

### Commands

```rust
pub struct CommandEnvelope {
    pub command_id: CommandId, // made by the client; a repeat returns the stored outcome
    pub command: HostCommand,  // {"<command>": {...}}
}
```

- No principal travels in a command. The surface that takes it (the desktop bridge, the
  transport, the cli) knows who sent it from its connection and passes the principal to the host
  itself, so a client cannot claim to be someone else; `PermissionResolved.by` comes from there.
- Commands refuse unknown fields and unknown commands, and never carry a file-system path: folders
  and prompt files are opaque ids, undo and restore name files by `MatterPath`.
- Typed fields are checked when they decode: `Title` for titles the lawyer types, `AgentChoiceId`
  for ids the agent chose, `MatterPath`, `AgentId` and the uuid ids. `CommandEnvelope::validate`
  checks the rest and returns a `ValidatedCommand`, which has private fields and no other
  constructor, so the host, which takes only that type, never runs an unchecked command, also one
  built in Rust; both fail as `command.invalid`. Events keep the agent's ids (`option_id`, `mode_id`, a
  select value) and titles as plain strings: the agent chooses them, and an event never fails to
  decode because an agent sent a long or empty one. In return `acp` refuses an offered permission
  option, mode or config option (or select value) whose id fails `AgentChoiceId::new` and fails
  that request with `agent.protocol_error`, so the UI never shows an offer no command can
  answer. `check_display_title` is public so `acp` and
  `host` can apply the title rules to agent text before it is shown.

Every limit and rule, with its constant:

| Where | Rule | Constant |
|---|---|---|
| `SendPrompt`, `EditQueuedPrompt` `content` | 1 to 100 blocks | `MAX_PROMPT_BLOCKS` |
| same | at most 256 KiB of text, UTF-8 bytes summed over `Text` blocks, not escaped | `MAX_PROMPT_TEXT_BYTES` |
| same | at most 50 `File` blocks | `MAX_PROMPT_FILES` |
| same | no empty `Text` block; a visible character (as titles count it) or a `File` block | |
| `SendPrompt.delivery` | a `DeliveryRequest`: closed, so a newer mode fails to decode, never run as another mode; queue is written as a missing field | |
| `Title` (`RenameThread.title`, `CreateMatter.name`) | at most 200 characters (`char`s) | `MAX_TITLE_CHARS` |
| same | a visible character: not only whitespace, `Default_Ignorable_Code_Point` characters, U+2800 and combining marks (a heuristic) | |
| same | no control character, bidi embedding, override or isolate (U+202A to U+202E, U+2066 to U+2069), or line or paragraph separator; the bidi marks U+200E, U+200F, U+061C are allowed | |
| `CreateMatter.name` with `NewFolder` | at most 255 UTF-8 bytes | `MAX_MATTER_NAME_BYTES` |
| same | not `.` or `..`, no leading dot (Finder hides the folder), none of `< > : " / \ \| ? *`, no leading space, no trailing dot or space | |
| same | not a Windows device name, in any case, with any extension, the stem's trailing spaces trimmed | `is_windows_reserved_name` |
| `CreateMatter.name` with `ExistingFolder` | a label: the title rules only; the desktop app shortens or cleans a folder name they refuse before pre-filling it | |
| `AgentChoiceId` (`AnswerPermission.option_id`, `SetConfigOption.option_id`, a select value, `SetMode.mode_id`) | 1 to 256 UTF-8 bytes | `MAX_ID_TEXT_BYTES` |
| `AnswerElicitation` with `Accept` | the values are a JSON object with string keys | |
| same | at most 64 KiB as compact JSON | `MAX_ELICITATION_VALUES_BYTES` |
| `UndoStep.files` | at least one `Undo` entry: a list of only `Keep` entries, or none, undoes nothing and is refused; files the list leaves out are kept | |
| `RestoreVersion` `Files` scope | at least one path | |
| `UndoStep.files`, `RestoreVersion` `Files` scope | at most 10,000 paths | `MAX_FILE_LIST` |
| same | at most 1 MiB of path bytes in all | `MAX_PATH_LIST_BYTES` |
| same | no path named twice, paths compared as bytes (the `MatterPath` rule) | |
| `UndoStep.files` | every path is in the step's `FilesChanged` (the host's check) | |
| `RestoreVersion.scope` | a `RestoreRequest`: closed, refuses unknown kinds and fields | |
| `MatterPath` | the rules under Envelope; at most 255 bytes per segment, 4096 in all | |
| `HostQuery::PageEntries.limit` | 1 to 200: a `PageLimit`, checked when the query decodes | `MAX_PAGE_ENTRIES` |
| `HostQuery::ReadBlob.len` | 1 to 1 MiB: a `ReadLen`, checked when the query decodes; `offset` at most 2^53 - 1 | `MAX_BLOB_READ_BYTES` |

### Serde conventions

- `rename_all = "snake_case"` for fields and variants, on the wire and on disk.
- Every enum with data, the envelope's `event` included, is externally tagged: the kind is the key of
  a one-key object (`{"text_appended": {...}}`, `{"inline": "<text>"}`). A unit variant is a plain
  string (`"pending"`). No `tag`, no `tag` with `content`: no internally or adjacently tagged enum.
  Measured on an Apple M6 Mac mini over 20,000 events: external tagging decodes 15 to 25% faster in
  Rust and parses 6 to 16% faster in the webview engines than the other shapes.
- `StreamId` and `EntryId` are one plain string, `<kind>:<uuid>`; the same text serves the wire, SQL
  and the command line. `StreamId` stays a string in MessagePack too, unlike the uuid ids, which
  are 16 bytes there: its text is the key SQL indexes and the command line names, so a binary
  form would shorten each row's `stream` by about 25 bytes and cost a conversion wherever the
  text is used as the key.
- A `BlobHash` is its 32 bytes in MessagePack and 64 lowercase hex characters in JSON, the form
  each format reads back. Its text is never a key and nobody types it into a log, so the bytes
  halve the hash in every row that holds one, and the JSON copy stays readable.
- Every `Option` field is left out when `None`, never written as `null`, and a missing field reads as
  `None`. Events accept fields they do not know, so an older reader takes a newer optional field.
  Commands refuse unknown fields.
- Four logged structs have fields that depend on each other and are built only through `new`,
  which refuses a combination that means nothing: `CheckpointRecorded` (`thread` and `turn` by
  `kind`), `FileChange` (`before` and `after` by `change`), `MatterCreated` (no `import` on
  the scratch matter) and `TurnQueued` (`behind` is the stream's thread when `behind_turn` is
  set). Decoding checks none of it, so an older or foreign entry still reads.
- Events decode in one pass: the kind is matched as a borrowed string and its payload decoded straight
  into its type. A known kind with a payload that does not decode is an error. The same holds in JSON
  and in MessagePack (the stored form).
- Nesting is bounded the same in both formats. A `RawPayload`, and the payload of an unknown kind
  under an open enum's catch-all, decodes up to `MAX_RAW_DEPTH` (100) nested lists and maps. A
  `RawPayload` encodes up to `MAX_RAW_ENCODE_DEPTH` (92), 8 levels less (see the freeze rules);
  an unknown event's payload or unknown content block, which a catch-all decoded, writes back
  under `MAX_RAW_DEPTH`, so what this version decoded still reaches the webview. A field an
  event does not know is skipped by serde, where `protocol` cannot count. `serde_json` skips it
  without recursing, so its depth does not matter. rmp-serde recurses into it, and plain
  `rmp_serde::from_slice` overflows a debug build's stack on about a kilobyte of nested lists, so
  every reader of stored or received MessagePack builds its `rmp_serde::Deserializer` with
  `set_max_depth(MAX_MSGPACK_DEPTH)` (128).
- Large variants are boxed; boxing does not change the JSON.

### Enums that decode unknown values

A newer Clauseo may add a variant to these; an older reader decodes it to the catch-all instead of
failing. This version never produces one, except `ContentBlock::Unsupported` for an ACP block kind
`acp` does not know, and the type enforces it: every `Unknown { kind }` with data, `DomainEvent`'s
included, holds its kind as an `UnknownKind`, which only `protocol`'s decoders build, so no other
crate can make a catch-all and log it. A unit `Unknown` writes `"unknown"`, which every version
reads back as `Unknown`.

- `DomainEvent::Unknown { kind, payload }`: the payload is kept as `RawPayload` and written back under
  its own kind as the same value; it still advances `seq`. JSON comes back as the same compact
  text but for floats, which come back within one ULP (`serde_json` without `float_roundtrip`).
  MessagePack comes back byte for byte only for what rmp-serde writes, canonical (shortest)
  integers and 64-bit floats: a wider integer comes back in the shortest form, a 32-bit float as
  64-bit. A few values change form, one-way, and no Clauseo writer produces any of them: a
  MessagePack ext value decodes as `Seq([U64(type), Bytes(data)])` and writes back as that array,
  a MessagePack str that is not valid UTF-8 decodes as `Bytes` and writes back as bin, and a JSON
  integer outside both `i64` and `u64` decodes as `F64` and writes back as a float. A
  `RawPayload` is exempt from `MAX_SAFE`: a `U64` above 2^53 (an elicitation schema's
  `maximum`, say) is written as its exact digits and reaches the webview as an inexact number.
  A `RawPayload` is always decoded from the frame's bytes, never converted from a
  `serde_json::Value`, which without `preserve_order` has already sorted its keys. Its JSON copy,
  when it was read from MessagePack, is a one-way projection: NaN and the infinities become
  `null`, which reads back as null, and a map key that is a scalar or binary data becomes a
  string holding the key's compact JSON text (`7` as `"7"`, as serde_json does; null and NaN
  as `"null"`; binary data as its list of byte numbers). That text holds no quote, so nothing is
  escaped twice. Such a key reads back as a string and collides with the string key of the same
  text: a NaN, infinity or null key with `"null"`, `true` and `false` with `"true"` and
  `"false"`, a number with its decimal string (`7` and `"7"`, `1.5` and `"1.5"`), binary data
  with the string of its byte list (`[1,2]` and `"[1,2]"`). The copy then holds the key twice,
  and a JSON reader that keeps the last duplicate (`serde_json::Value`, JS `JSON.parse`)
  silently drops the earlier entries. A key that is a list or a map has no JSON form, and
  decoding refuses it in both formats (see the freeze rules), so the row fails at the store read
  and a decoded `RawPayload` always has a JSON copy; only a value built by hand with such a key
  fails to encode as JSON, with `Error::NotJson`. MessagePack keeps a scalar or binary key as it is.
  Every `RawPayload` (an elicitation schema too) and a content block's `raw` text do the same.
  A command is strict input, so `AnswerElicitation` still refuses values with a key that is not
  a string.
- `ContentBlock::Unsupported { kind, raw_from, raw }`: the whole unknown block kept as compact JSON.
  When this version's decoder captures it (`raw_from: clauseo`), `raw` is the block as read, written
  as JSON: a block read from a MessagePack row writes its binary values (uuids) as JSON arrays of
  numbers, where the wire form has strings, so it is not Clauseo's JSON form. An upcaster reading
  `raw_from: clauseo` must accept both forms. Its payload is read one level down, as it sits in
  the whole block, so 99 nested lists decode and 100 fail to decode the event, as the whole block
  would fail to encode under `MAX_RAW_DEPTH`. A block holding a list or a map as a map key fails
  to decode the event too. A scalar or binary map key never fails the event: it keeps `raw` with
  the key as its JSON text. `raw` is kept only when its `escaped_len` is at most
  `INLINE_LIMIT_BYTES`, else it is `None`: the block lands in an `AgentContent` or
  `UserMessage` entry, whose inline parts share that limit and whose `Upsert` cannot be split,
  and JSON text inside a JSON string escapes every quote and backslash again. The log keeps the
  original event either way. This follows from
  holding the captured value as `RawPayload` rather than a `serde_json::Value`, which cannot read
  MessagePack's binary values.
- `ErrorCode::Unknown`: the original code moves into `HostError.message`, and into the front of
  `TurnFailed.detail` (0003).
- Unit-only enums with a unit `Unknown` (`open_unit_enum!`): every unit enum marked open in the
  table under Freeze rules but `ErrorCode`, which its own table writes.
- Enums with data, with `Unknown { kind }` (`open_data_enum!`; the payload is dropped in memory,
  the log keeps the original event): every data enum marked open in that table but two,
  `ContentBlock`, whose catch-all is its own row `Unsupported`, and `DomainEvent`, which
  `domain_events!` writes; and `LimitSource`, which is not logged.
- An open enum accepts fields it does not know, as every logged type does, so none appears in a
  command, which refuses them. `HostCommand::RestoreVersion` names its scope with
  `RestoreRequest`, a closed twin of the logged `RestoreScope` with the same wire form and
  `deny_unknown_fields`; the host converts it with `From` before logging `version_restored`.
  `HostCommand::SendPrompt` names its delivery the same way, with `DeliveryRequest`, the closed
  twin of `Delivery`, so a newer mode fails to decode as `command.invalid`. Queue has one wire
  form in the command, `UserMessage` and `UserMessageEntry`: a missing `delivery`, which is how
  queue is written and how an old row without the field reads.
- A decoded `Unknown { kind }` writes back as the plain string `kind`, without its payload. It goes
  to readers of this version or older (the webview, re-sent copies of an event) and never back to
  the newer peer that wrote it, which refuses a plain string for a data kind.
- Serialising an `Unknown` whose kind is one this version knows is an error, so a value that would
  not decode again never reaches the log.
- Each open unit-only enum's `ALL` lists every value, and each open enum with data's
  `VARIANT_NAMES` the Rust name of every variant, the catch-all last in both. The macro that writes
  the enum also writes its test, in JSON and MessagePack: `open_unit_enum!` round-trips every value
  of `ALL`; `open_data_enum!` takes one sample value per variant, fails when the samples miss one,
  and round-trips each. Both read an unknown kind to the catch-all. A variant the decoder does not
  handle fails the build's tests instead of coming back as the catch-all.

### Inline limit

`TextOrBlob` is `Inline(String)` or `Blob(BlobRef { hash, size_bytes, preview })`, with a BLAKE3 hash
and a preview of at most `PREVIEW_BYTES` (512) that is no longer than the blob. The inline parts of one view entry cost at most
`INLINE_LIMIT_BYTES` (4096): UTF-8 bytes after JSON escaping, without the quotes, summed over the
entry's `Inline` values, `Vec` items included. Fields are taken in declaration order: for a tool call
`raw_input`, `raw_output`, then `content` items; for a user message its `content` items; for agent
content its block; for a compaction marker its `summary`. A text patch costs its text. `escaped_len` and
`TextOrBlob::inline_len` are the only way to count it; the host enforces it when it builds the event.
A decoded `ContentBlock::Unsupported` block counts its `raw` copy. The decoder keeps a block's `raw`
only when it fits `INLINE_LIMIT_BYTES` alone, but an entry may hold several unknown blocks (a user
message's or a tool call's `content`), so the reducer drops `raw` from the entry's unknown blocks,
last block first, until the entry's inline parts fit `INLINE_LIMIT_BYTES`; the log keeps the
original event.
Message text stays a `String` and streams as `AppendText` patches under the same limit: no patch
carries more than `INLINE_LIMIT_BYTES` of message text. A long agent text goes out as an
`Upsert` with its `text` cut, then `AppendText` patches. A user message's `Text` blocks are not
`Inline` values; they have their own cap. The `Upsert` of a user message (a prompt holds up to
256 KiB of text) carries at most `INLINE_LIMIT_BYTES` summed over all its `Text` blocks, each
counted as `escaped_len` counts it. The blocks are taken in order: each keeps as much of its text
as the room left by the blocks before it allows, so a block past the limit comes as empty text.
`AppendText { target: UserBlock(i) }` patches then add the rest of each cut block.

### Freeze rules

`schema_version` versions the stored log: `Envelope` and `DomainEvent`, with every type they hold.

- Open or closed, decided once. An enum reachable from a logged type (`DomainEvent` and
  everything in it) is open: it decodes a value it does not know to an `Unknown` catch-all and
  keeps it, so a new variant is additive. It is closed only when it is structural: its set is
  fixed by Clauseo's own storage format, so growing it needs a format change anyway. The table
  below classifies every enum in a logged type; a new one gets a row, and a closed one a reason.
  `tests/enum_table.rs` (built with `--features specta`) enforces it: it walks the types the
  TypeScript export reaches from `Envelope` and `DomainEvent`, and fails when an enum there has
  no row, a row names no such enum, or a row's class is not what the type does (an open enum
  decodes an unknown kind to its catch-all, a closed one refuses it, and `RawPayload` reads it as
  plain data). `KnownDomainEvent`, which the export adds, is no enum of its own and has no row.
- No variant of a logged enum wraps a bare non-struct value (`TextOrBlob`, `ContentBlock`, a `Vec`,
  a number). A variant's payload is always written as a map: a variant is a unit, a struct variant,
  or a newtype over a struct with named fields of its own, as `DomainEvent`'s variants are, never
  over a unit or tuple struct, an `Option` or a `#[serde(transparent)]` wrapper (each can write
  `null` or a list, which a reader refuses under a known kind). A variant never changes shape: a
  unit variant stays a plain string and a struct variant a one-key map, and a variant gains a field
  only as an `Option` or with `#[serde(default)]`. A variant that needs fields it lacks is added
  as a new variant instead, which an older reader decodes to the catch-all (it could not honour
  the new fields anyway). A known kind in the other form is an error that names the kind: a unit
  kind as the key of a one-key map, whatever its payload, `{}` and `null` included
  (`` `local_desktop` has no fields, so it is a plain string, not a map ``), and a struct kind as a
  plain string (`` `files` has fields, so it is a one-key map, not a plain string ``) or with a
  payload that is neither its map nor its fields as a list, which JSON and MessagePack both read
  by position. An unknown kind is the catch-all as a plain string and as a key with
  any payload, `null` included, skipped with its nesting counted, in a unit-only enum as in an enum
  with data: `{"zz": 5}` is `PlanPriority::Unknown` and `Principal::Unknown { kind: "zz" }`. This
  holds in JSON and MessagePack for every enum `open_unit_enum!` or `open_data_enum!` writes
  (`ErrorCode`, a code, stays a plain string).
  `ToolCallContent` holds its
  text as `Text { text }`, the field named as in `ContentBlock::Text` and ACP's text block, and
  its other blocks as `Content { content }`, ACP's `{"type": "content", "content": ..}`;
  `RestoreScope` names its files `Files { paths }`. `ToolCallContent::Diff` names its file with
  a `ToolLocation`, not a `MatterPath`: ACP's diff carries any absolute path, a tool may edit a
  file outside the matter, and `ToolLocation::Outside` keeps that one as display text the UI
  flags, as a tool call's locations do. The structural enums are the exception: their
  variants are the storage format itself.

| Enum | Open or closed | Shape | Why closed |
|---|---|---|---|
| `DomainEvent` | open, `Unknown { kind, payload }` | data | |
| `ErrorCode` | open, `Unknown` | unit | |
| `ContentBlock` | open, `Unsupported { kind, raw_from, raw }` | data | |
| `DetachReason`, `SteerOutcome`, `Principal`, `ToolCallContent`, `ToolLocation`, `PermissionChoice`, `ElicitationRequest`, `ConfigOptionKind`, `ChangeKind`, `RestoreScope` | open, `Unknown { kind }` | data | |
| `ToolKind`, `ToolCallStatus`, `PermissionKind`, `StopReason`, `NoticeLevel`, `Deliverable`, `CheckpointKind`, `ExpiryReason`, `RenameSource`, `Delivery`, `SkippedFileReason`, `TextKind`, `RawFrom`, `ElicitationOutcome`, `PlanPriority`, `PlanStatus`, `ConfigCategory`, `CompactionStatus`, `LimitStopReason`, `MatterKind` | open, `Unknown` | unit | |
| `TextOrBlob` | closed | data | Inline text or a blob reference is how Clauseo stores a payload; a third form is a new storage format. |
| `RawPayload` | closed | data | It is the data model of JSON and MessagePack themselves. |
| `StreamId` | closed | data (one string) | A thread's or a matter's stream is the log's own keying; a new stream kind changes the store. |

- No Clauseo type uses a list or a map as a map key, so a newer Clauseo's MessagePack never holds
  one, and such a key in a row means the row is corrupt or hostile. Its JSON copy has no form,
  so decoding refuses it and the row fails at the store read, as a too-deep one does; an unknown
  event or content block holding one fails to decode the event. Scalar and binary keys, which
  a map keyed by an id uses, decode and are written as their JSON text.
- Every version encodes a `RawPayload` at most `MAX_RAW_ENCODE_DEPTH` (`MAX_RAW_DEPTH - 8`, 92)
  lists and maps deep, and decodes up to `MAX_RAW_DEPTH` (100); `RawPayload`'s encoder enforces
  the lower limit, so a producer's deeper value fails when it commits. An older reader counts
  from the root of the payload under its catch-all, so every map a newer version wraps around a
  `RawPayload` costs a level there: a new kind's payload struct, a new enum variant's struct, a
  struct variant in a content block, whose whole block `raw` keeps adds one more. The 8 levels
  absorb that wrapping; a newer version keeps every `RawPayload` it adds within 8 levels of its
  catch-all's root, so its rows decode here. A row newer than the reader reads as
  `EnvelopeRead::Newer` in either key order whatever its event holds, within the format's
  nesting limit. A later version never raises
  `MAX_RAW_ENCODE_DEPTH` (an older reader would refuse its rows) or lowers `MAX_RAW_DEPTH` (it
  would refuse an older version's rows).
- Log types. The envelope's own fields (`schema_version`, `event_id`, `stream`, `seq`, `ts`,
  `command_id`) are frozen: no version renames, removes or retypes one, since every version
  decodes them with its own types and `upcast` takes only the event. `schema_version` stays the
  first field written, so a reader knows the version before it reads the event. Inside the event,
  additive changes keep `schema_version`: a new kind, a new `Option` field that is left out when
  `None`, a new field with `#[serde(default)]` (an older row without it reads the default), or a
  new variant in one of the enums that decode unknown values. Each still needs a decision record
  and the code owner's approval. Only a change inside the event bumps `schema_version`: renaming,
  removing or retyping anything the event holds, or a new variant in a closed enum. The bump adds
  the step from the version before to `upcast` in `protocol`'s `envelope.rs`, which turns the
  older event, read as `RawPayload`, into this version's `DomainEvent`.
- Types outside the log. A new `HostCommand` or `HostQuery` variant (client to host only; an older
  host refuses it as `command.invalid`), or a new `QueryResult`, `HostEvent`, `ViewPatchBody` or
  `ViewEntry` variant, is additive: it needs a decision record and the code owner's approval, and no
  `schema_version` bump. Renaming, removing or retyping one of these needs the same, plus a plan for
  the clients that use it.
- "Additive" for view and client-to-host types holds only because the desktop app and the cli
  ship with the host, so both ends change together. `ThreadStatus`, `ViewEntry`, `ViewPatchBody`,
  `HostEvent` and most other enums there have no catch-all: an older cli or a phone client fails
  on a new `ThreadStatus` or `ViewEntry` kind (`"status":"held"` gives `unknown variant`). A
  client released apart from the host needs a version check or a catch-all first.
- `store` keeps three of those types outside the log, and none of them is versioned by
  `schema_version`: view entries in `reducer`'s snapshots, `AccountLimits` in the limits cache
  (one per account), and `CommandOutcome` in receipts. They hold closed enums (`ThreadStatus`,
  `MarkerKind`, `StaleReason`, `UnavailableReason` and more), so after a downgrade or a new
  variant a stored value can stop decoding. The rule:
  - Snapshots and the limits cache are disposable and keyed by the build that wrote them. A build
    mismatch or a value that does not decode means rebuild the snapshot from the log, or read the
    limits again from their source. An unknown `StaleReason` inside a cached `LimitSource` fails
    the whole cached value, and the store reads the limits again.
  - A receipt that does not decode counts as missing, which allows one re-run per `command_id`:
    the command runs again once and writes a new receipt.
  - So an open enum's catch-all written back in its plain form is safe in a snapshot.
    `{"crashed":{"signal":9}}`, read by an older build as `DetachReason`'s `Unknown` and written
    back as `"crashed"`, fails to decode in a newer build, which refuses a plain string for a data
    kind; that build rebuilds the snapshot from the log, where the event keeps its full form.
- Golden snapshots of one canonical example per kind, view patch, host event, command, view entry
  and content variant pin the JSON, and each example decodes back to itself; a changed snapshot
  is a schema change.
- A frozen corpus guards old entries, since an author accepts a changed snapshot as routine.
  `crates/protocol/tests/corpus/v1/` holds log entries as written at `schema_version` 1, in JSON
  (`events.jsonl`) and in MessagePack (`events.msgpack`): every kind and every variant of every
  open enum in this table, data or unit, each at least once, with every optional field filled
  somewhere. `crates/protocol/tests/corpus.rs` decodes both with the current types and fails on
  an entry that no longer decodes or reads as `Unknown`, on a stored field that decoding and
  writing back drops, and on a kind or an open enum's variant missing from the corpus. A new
  field without `Option` or `#[serde(default)]`, in an event or in any variant, a renamed field
  or a removed kind fails there. That every optional field is filled was checked once, when v1
  was frozen, and no test repeats it, since a field added later is rightly absent. The files are
  never edited or regenerated, and insta never touches them: the test pins each file's length
  and FNV-1a hash. A kind, or a variant of an open enum, added without a `schema_version` bump
  brings its own frozen entries in `corpus/added/`, listed in the test's `ADDED_AFTER_V1`; the
  known kinds must be v1's plus the added ones, so a removed kind fails and an added one without
  its entry fails too. A unit value counts for every enum its string decodes as, so a new unit
  variant spelled like a value of another enum that the corpus holds passes unseen. A later
  schema adds `v2/` beside v1, and v1 keeps decoding. The steps for adding a pair, and the
  command that writes it, are in the
  [corpus README](../../crates/protocol/tests/corpus/v1/README.md).

#### Checklist: adding a kind, a variant or an open enum

Each step names what fails when it is missed; a step marked unchecked fails nothing, so the
author and the reviewer check it by hand. Paths are under `crates/protocol/`. Each addition also
needs a decision record and the code owner's approval (Log types above), which no test checks.

A new kind:

1. The payload struct, in its subject's file in `src/event/payload/`, re-exported in
   `src/event/payload.rs`, in `src/event.rs`'s `pub use payload::{..}` and in the `event::` group
   of `wire_types!` in `src/lib.rs`. The crate does not build without the first two once the row
   below names the struct, and the tests do not build without the third, since
   `tests/support/examples.rs` imports the struct from the crate root.
2. Its row in `domain_events!` in `src/event.rs`, `Variant(Payload) = "kind", thread;` (or
   `matter;`), placed where the kind goes in `DomainEvent::KINDS`. The macro writes the variant,
   `KINDS`, `kind()`, the stream rule and the decoder from it; the steps below fail until they
   follow it.
3. Its example in `every_event()` in `tests/support/examples.rs`, in `KINDS` order:
   `kinds_equal_known_kinds` (tests/snapshots.rs).
4. Its golden row in `golden! { DomainEvent::name_of {..} }` in `tests/snapshots.rs`: the tests do
   not build without it, since `name_of` matches every variant with no wildcard arm. Then its
   `.snap` in `tests/snapshots/`: the new golden test fails until the snapshot is reviewed and
   accepted (`cargo insta test -p clauseo-protocol --accept`).
5. Its row in the kind table under Kinds above, Logged `yes` and Stream `T` or `M`, in `KINDS`
   order: `kind_table_matches_record` (tests/events/rules.rs).
6. Its frozen entries: the pair `<kind>.jsonl` and `<kind>.msgpack` in `tests/corpus/added/` and
   its row in `ADDED_AFTER_V1` in `tests/corpus.rs`. The ignored `write_the_added_pairs` writes
   the pair from step 3's example into `target/tmp/corpus-added/` and prints the `mv` commands
   and the row; `tests/corpus/v1/README.md` (Adding a kind or a variant after v1) gives the
   command: `the_known_kinds_are_v1_and_the_added_ones`.

A new variant of an open enum:

1. The variant in the enum's macro call. `open_data_enum!` also needs its sample value in the
   `test mod <name> with [..]` line: the test the macro writes fails when the samples miss a
   variant. `open_unit_enum!` needs nothing more, since its test round-trips every value of the
   `ALL` the macro writes.
2. If `tests/events/unknown_variants.rs` checks the enum with `unit_enum`, its
   `(Variant, "wire_name")` case, in `ALL` order: that test asserts the cases follow `ALL`.
3. If the enum is one of the 15 that `every_listed_variant_is_in_ui_states` (tests/ui_states.rs)
   lists, `ThreadStatus`, `ToolKind` and `ErrorCode` among them, a row naming `Type::Variant` in
   the second column of a table in sections 1 to 9 of `docs/ui-states.md`: that test. The same
   holds for a new variant of a closed enum on that list.
4. If the enum is logged, an example in `tests/support/examples.rs` that holds the variant, and
   its frozen entries: the pair `<kind>-<Variant>.jsonl` and `.msgpack` in `tests/corpus/added/`
   and its `ADDED_AFTER_V1` row, which `write_the_added_pairs` writes from that example as in
   step 6 of a new kind: `the_corpus_holds_every_variant_of_every_open_enum` (tests/corpus.rs).
   A unit value spelled like one the corpus holds for another enum passes that test unseen, so
   the pair is still added by hand.
5. The enum table under Freeze rules lists enums, each with its catch-all, not variants, so a new
   variant leaves it as it is.

A new open enum:

1. The macro: `open_unit_enum!` when every variant is a unit, `open_data_enum!` when any carries
   data, with one sample value per variant in its `test mod` line. The crate does not build
   without that line. The enum goes in its module's group of `wire_types!` in `src/lib.rs`, as
   in step 1 of a new kind.
2. Its row in the enum table under Freeze rules (open, its catch-all, `unit` or `data`):
   `the_table_has_one_row_per_enum_with_its_class` and `each_class_is_what_the_type_does`
   (tests/enum_table.rs), and `the_corpus_holds_every_variant_of_every_open_enum`, which
   compares the table's open enums with its own lists.
3. Its entry in `logged_enums!`, the one list of enums in `tests/support/enums.rs`: under
   `data:` as `Name(CatchAll)` or under `unit:` as `Name`. The list feeds `enum_table.rs`'s rows
   and `corpus.rs`'s open-enum lists: `the_table_has_one_row_per_enum_with_its_class` fails when
   the table and the list differ, and `rows_are_the_reachable_enums` (tests/enum_table.rs, built
   with `--features specta`) when an enum reachable from `Envelope` or `DomainEvent` is missing
   from the list.
4. A test in `tests/events/unknown_variants.rs`, `unit_enum` for a unit-only enum and
   `data_enum_unknown` for one with data: unchecked.
5. If a screen shows the enum, its entry in `every_listed_variant_is_in_ui_states` and a row per
   variant in `docs/ui-states.md`: unchecked.
6. Each variant's frozen entries, as in step 4 of a new variant.

### Dependency row

`ARCHITECTURE.md`'s `protocol` row becomes `exists`, and its "May depend on" grows from serde, uuid
and specta to serde, serde_json, uuid, compact_str, thiserror, specta and specta-serde.

- `serde_json` 1.0.154, no features: it writes `Unsupported.raw` as compact JSON and
  backs the escaped-length tests; a JSON writer of our own would be one more place to get escaping
  wrong. sonic-rs and simd-json were measured on an Apple M6 Mac mini and rejected: sonic-rs encodes
  26 to 59% faster but pulls in 30 crates against 5 and has 250 `unsafe` sites against 16, and
  simd-json is slower on small inputs.
- `compact_str` 0.10.0, with `serde`: short strings (ids, titles, tool names, MIME types, option ids)
  need no heap block up to 24 bytes; cloning is 85% faster on an Apple M6 Mac mini. `protocol`
  re-exports `CompactString`.
- `thiserror` 2.0.21: derives `Display` and `std::error::Error` for `Error` and `HostError`; it
  is a derive macro and adds no code that runs.
- `specta` stays optional, behind a feature only the app turns on, with `specta-serde` 0.0.12 (the
  one for specta 2.0.0-rc.25), which applies the serde attributes. The feature gives the app its
  TypeScript export: `wire_types()` registers every wire type (the one list is the `wire_types!`
  call in `lib.rs`; `DomainEvent` brings `KnownDomainEvent`, its known kinds without the
  unknown arm, which the webview narrows to), and `WireFormat` exports them as their JSON form, a
  left-out field as `field?: T` and a `#[serde(other)]` catch-all as `"unknown"`;
  `tests/specta_export.rs` checks the output. "Never depends on" is unchanged: tauri, tokio, fs,
  process.

## Consequences

- `store`, `reducer`, `host`, `acp`, `cli` and the webview share one envelope, one kind table and one
  text form for streams and entries. A Versions timeline is a fold of the matter's stream like any
  other view.
- An older Clauseo reads a newer log: unknown kinds and variants decode to their catch-all variant and unknown
  optional fields are skipped, so additive changes need no migration.
- Any change to events, commands, view patches or error codes after this record needs a new decision
  record and the code owner's approval; a breaking one also needs an upcaster.
- Message text stays inline and unbounded per message; only agent payloads that land in one entry go
  to the blob store.

## Status

accepted

## Date

2026-10-22
