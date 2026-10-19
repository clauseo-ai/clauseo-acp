# UI states

Every state the screens must design for, and where its data comes from. Each row names the state, the
type and variant that produce it (or the M2 crate that will add it), and the data the screen gets.
This file holds engineering facts only; design writes the copy. The `every_listed_variant_is_in_ui_states`
test in `crates/protocol` checks that every variant of `ThreadStatus`, `StopReason`, `ToolKind`,
`ToolCallStatus`, `PermissionKind`, `ExpiryReason`, `ElicitationOutcome`, `CheckpointKind`,
`SkippedFileReason`, `LimitSource`, `Delivery`, `SteerOutcome` and `ErrorCode` appears here in
backticks, written as `StopReason::EndTurn` is.

A variant named `Unknown` is a value from a newer Clauseo that this version does not know. It is
never produced by this version, so each needs only a neutral fallback.

Wire names are snake case (`at_limit`, `send_now`); the tables use the Rust names. The event schema is
in [0002](decisions/0002-event-schema.md), the error codes in [0003](decisions/0003-error-codes.md).

## 1. Thread status

Shown as the sidebar dot (`ThreadSummary.status`) and in the thread header (`HeaderEntry`).

| State | Type and variant | Data the screen gets |
|---|---|---|
| Idle, ready for a prompt | `ThreadStatus::Idle` | title, agent, mode, config options, commands, plan, context usage (`ContextUsage`) |
| Agent starting | `ThreadStatus::Starting` | as idle; the agent has no session yet |
| Turn running | `ThreadStatus::Running` | as idle; also while prompts of the same thread wait in its queue |
| Needs you: a permission or elicitation is pending | `ThreadStatus::NeedsYou` | the pending `ViewEntry::Permission` or `ViewEntry::Elicitation` |
| Waiting for another thread in the matter to finish | `ThreadStatus::Queued` | `behind`: the thread holding the matter; its title from `ThreadSummary` |
| Paused at the usage limit | `ThreadStatus::PausedAtLimit` | `resets_at` when known; section 8 |
| Agent exited | `ThreadStatus::AgentExited` | the status only (the exit code is in the log's `DetachReason::Exited`, not in a view entry; the stderr tail is never shown). Restart action: `HostCommand::RestartAgent`, refused with `ErrorCode::ThreadTurnRunning` during a turn |
| Agent stopped while idle (M2 emits it) | `ThreadStatus::IdleStopped` | the status only (logged as `DetachReason::IdleStopped`); resuming is in section 10 |
| Archived | `ThreadStatus::Archived` | title, agent; read-only |

Detach reasons are in the log only (`agent_session_detached`, read through `SubscribeEvents`); no view
entry carries them in M1: `DetachReason::Exited`, `DetachReason::Closed`, `DetachReason::Restarting`,
`DetachReason::IdleStopped`, and `DetachReason::Unknown`.

## 2. Transcript entries

Entries sort by `order`. Text streams through `ViewPatchBody::AppendText`; everything else arrives as
`ViewPatchBody::Upsert`. Opening a thread, or a lost cursor, sends `ViewPatchBody::Reset` with a
`ViewSnapshot`: the header, about 200 recent entries, and `has_older` when older entries exist. Older
entries page in on scroll through `HostQuery::PageEntries`, which returns `QueryResult::Entries`.

| State | Type and variant | Data the screen gets |
|---|---|---|
| User message, sent | `ViewEntry::UserMessage` with `UserMessageStatus::Sent`, `Delivery::Queue` | content blocks (text, attached files), turn, delivery |
| User message, queued behind a running turn | `UserMessageStatus::Queued` | content, turn; edit, remove and send now in section 5 |
| User message, edited while queued | `UserMessageStatus::Queued` with `edited` | the new content; same turn, same place |
| User message, removed from the queue or cancelled before it started | `UserMessageStatus::Removed` | the text stays, so it can be sent again |
| User message, sent now | `Delivery::SendNow` | content; the running turn ends `StopReason::Cancelled` |
| User message, steered | `Delivery::Steer` | content; then the entry's `steer` holds one outcome below |
| Steer taken into the running turn | `SteerOutcome::Injected` | the turn that answers it |
| Steer moved to the next turn (the turn had just ended) | `SteerOutcome::Requeued` | the new turn, which starts next |
| Steer sent as send now (no real steer for this agent) | `SteerOutcome::SentNow` | the new turn; the running one is cancelled |
| Steer outcome from a newer Clauseo | `SteerOutcome::Unknown` | the outcome's kind only |
| Answer, streaming | `ViewEntry::AssistantMessage`, `TextKind::Answer` | text so far, growing by `AppendText` |
| Answer, complete | `ViewEntry::AssistantMessage` | full text; the turn has ended |
| Thought, streaming and complete | `ViewEntry::AssistantMessage`, `TextKind::Thought` | as the answer |
| Agent image | `ViewEntry::AgentContent` with `ContentBlock::Image` | `data` inline, or `TextOrBlob::Blob` (preview, size; full data by `HostQuery::ReadBlob`); MIME type |
| Agent audio | `ContentBlock::Audio` | data (inline or blob), MIME type |
| Agent embedded file | `ContentBlock::Resource` | URI, MIME type, text or blob |
| Agent link to a file | `ContentBlock::ResourceLink` | URI, name, title, description, size |
| Agent block this version cannot show | `ContentBlock::Unsupported` | the block's kind; `RawFrom::Acp` (an ACP kind) or `RawFrom::Clauseo` (a newer Clauseo) |
| Tool call: read | `ToolKind::Read` | title, locations, status, content |
| Tool call: edit | `ToolKind::Edit` | as above; diffs in content |
| Tool call: delete | `ToolKind::Delete` | as above |
| Tool call: move | `ToolKind::Move` | as above |
| Tool call: search | `ToolKind::Search` | as above |
| Tool call: run a command | `ToolKind::Execute` | as above; raw input (the command) |
| Tool call: think | `ToolKind::Think` | as above |
| Tool call: fetch from the web | `ToolKind::Fetch` | as above |
| Tool call: switch mode | `ToolKind::SwitchMode` | as above |
| Tool call: other | `ToolKind::Other` | as above; `tool_name` when the agent gave one |
| Tool call of a kind from a newer Clauseo | `ToolKind::Unknown` | as other |
| Tool call waiting to run | `ToolCallStatus::Pending` | `ViewEntry::ToolCall` fields |
| Tool call running | `ToolCallStatus::InProgress` | as above |
| Tool call done | `ToolCallStatus::Completed` | as above; raw output |
| Tool call failed | `ToolCallStatus::Failed` | as above |
| Tool call cancelled with its turn | `ToolCallStatus::Cancelled` | as above |
| Tool call status from a newer Clauseo | `ToolCallStatus::Unknown` | as above |
| Tool output text | `ToolCallContent::Text` | inline text, or a blob shown by its preview and size |
| Tool output block | `ToolCallContent::Content` | any `ContentBlock` above |
| Tool diff | `ToolCallContent::Diff` | location, old text (none for a new file), new text; each inline or blob |
| Large payload shown by its preview | `TextOrBlob::Blob` | `BlobRef`: preview (at most 512 bytes), size; the rest by `HostQuery::ReadBlob` |
| Location inside the matter | `ToolLocation::Inside` | matter path, line |
| Location outside the matter (flagged) | `ToolLocation::Outside` | display text only |
| Plan entries by priority | `PlanPriority::High`, `PlanPriority::Medium`, `PlanPriority::Low` | entry text, in the header's plan |
| Plan entries by status | `PlanStatus::Pending`, `PlanStatus::InProgress`, `PlanStatus::Completed` | as above |
| Notice, information | `ViewEntry::Notice` with `NoticeLevel::Info` | text |
| Notice, warning | `NoticeLevel::Warning` | text |
| Notice level from a newer Clauseo | `NoticeLevel::Unknown` | text |
| Compaction started and completed | `ViewEntry::Marker` with `MarkerKind::Compaction`: `CompactionStatus::Started`, `CompactionStatus::Completed` | summary (inline or blob) when completed |
| Undo marker | `ViewEntry::Marker` with `MarkerKind::StepUndone` | the step's turn, the new checkpoint, restored paths, kept paths |
| Restore marker | `ViewEntry::Marker` with `MarkerKind::VersionRestored` | the version restored from, the new checkpoint, scope |

## 3. Step card

One `ViewEntry::Step` per ended turn: stop reason, the failure code (`failure`) when it failed, files
changed, skipped files, the after-turn checkpoint or none, `undo_possible`, `undone` once the step was
undone, and token usage (`usage`: total, input, output, thought, cached) when the agent reported it.

| State | Type and variant | Data the screen gets |
|---|---|---|
| Finished normally | `StopReason::EndTurn` | files changed; the queue moves on |
| Stopped at the token limit | `StopReason::MaxTokens` | files changed; the queue is held |
| Stopped at the agent's turn-request limit | `StopReason::MaxTurnRequests` | as above |
| Agent refused | `StopReason::Refusal` | as above |
| Cancelled (by the lawyer or by send now) | `StopReason::Cancelled` | as above; a queued prompt removed before it started gets no step card, only `UserMessageStatus::Removed` |
| Stopped at the usage limit | `StopReason::AtLimit` | as above; the `ViewEntry::LimitStop` card in section 8 |
| Failed | `StopReason::Failed` | `failure`: the error code (section 9); the event's detail is never shown |
| Stop reason from a newer Clauseo | `StopReason::Unknown` | as failed, without a code |
| File added | `ChangeKind::Added` | path, size, after content id |
| File modified | `ChangeKind::Modified` | path, size, before and after content ids |
| File deleted | `ChangeKind::Deleted` | path, size, before content id |
| File renamed | `ChangeKind::Renamed` | old and new path |
| File may include the lawyer's edits | `FileChange.may_include_lawyer_edits` | unticked by default in the undo choice |
| File skipped: over the size cap | `SkippedFileReason::OverSizeCap` | path |
| File skipped: cloud-only | `SkippedFileReason::CloudOnly` | path |
| File skipped for a reason from a newer Clauseo | `SkippedFileReason::Unknown` | path |
| Checkpoint missing, undo impossible | no `checkpoint_after`, `undo_possible` false | shown, never silent |
| Step already undone | `undone` true | the undo marker (section 2) |
| Undo disabled while a turn runs in this thread | `undo_possible` false | the step |
| Undo refused: a turn runs in another thread of the matter | the host checks when the command arrives: `ErrorCode::MatterTurnRunning` | the code only (retryable) |
| Undo with a choice per file | `HostCommand::UndoStep` with `FileChoice::Undo`, `FileChoice::Keep` | the step's files |

## 4. Approvals

| State | Type and variant | Data the screen gets |
|---|---|---|
| Permission option: allow once | `PermissionKind::AllowOnce` | option id; the agent's label as secondary text |
| Permission option: always allow | `PermissionKind::AllowAlways` | as above |
| Permission option: reject once | `PermissionKind::RejectOnce` | as above |
| Permission option: always reject | `PermissionKind::RejectAlways` | as above |
| Permission option of a kind from a newer Clauseo | `PermissionKind::Unknown` | the agent's label only |
| Permission pending | `ViewEntry::Permission` with `PermissionState::Pending` | options, the tool call it is for, deliverable |
| Permission the agent is waiting on | `Deliverable::Live` | as above |
| Permission that can no longer reach the agent | `Deliverable::NotResumable` | as above; answering is not possible |
| Deliverability from a newer Clauseo | `Deliverable::Unknown` | as not resumable |
| Tool call touches a location outside the matter | `ToolLocation::Outside` on the `ViewEntry::ToolCall` the permission's `tool_call` names | display text, flagged |
| Permission answered | `PermissionState::Resolved` with `PermissionChoice::Selected` | option id and kind, by whom (`Principal::LocalDesktop`) |
| Permission cancelled | `PermissionState::Resolved` with `PermissionChoice::Cancelled` | by whom |
| Permission expired | `PermissionState::Expired` | one reason below |
| Permission expired: the agent exited | `ExpiryReason::AgentExited` | the request |
| Permission expired: the turn ended | `ExpiryReason::TurnEnded` | the request |
| Permission expired: the host restarted | `ExpiryReason::HostRestarted` | the request |
| Expiry reason from a newer Clauseo | `ExpiryReason::Unknown` | the request |
| Answer came too late or twice | `ErrorCode::PermissionNotPending` | the entry's current state |
| Answer names an option not offered | `ErrorCode::PermissionUnknownOption` | the request |
| Elicitation pending | `ViewEntry::Elicitation` with `ElicitationState::Pending` | the request below |
| Elicitation form | `ElicitationRequest::Form` | message; schema with string, number, boolean and enum fields |
| Elicitation link | `ElicitationRequest::Url` | message, URL |
| Elicitation answered | `ElicitationState::Resolved` | outcome below, by whom |
| Elicitation accepted | `ElicitationOutcome::Accepted` | by whom; the values are never logged |
| Elicitation declined | `ElicitationOutcome::Declined` | by whom |
| Elicitation cancelled | `ElicitationOutcome::Cancelled` | by whom |
| Elicitation expired | `ElicitationState::Expired`: `ExpiryReason::AgentExited`, `ExpiryReason::TurnEnded`, `ExpiryReason::HostRestarted`, `ExpiryReason::Unknown` | as for permissions |
| Elicitation answered too late | `ErrorCode::ElicitationNotPending` | the entry's current state |
| Elicitation values do not match the schema | `ErrorCode::ElicitationInvalidValues` | the form, to correct |

## 5. Composer

| State | Type and variant | Data the screen gets |
|---|---|---|
| Config option, select | `ConfigOptionKind::Select` | id, name, current value, choices (with groups) |
| Config option, on or off | `ConfigOptionKind::Boolean` | id, name, current value |
| Config option categories | `ConfigCategory::Mode`, `ConfigCategory::Model`, `ConfigCategory::ThoughtLevel`, `ConfigCategory::Other` | set through `HostCommand::SetConfigOption`; `HostCommand::SetMode` when the agent has no mode option |
| Slash commands | `AvailableCommand` | name, description, input hint |
| Send, queued when a turn runs | `Delivery::Queue` | `HostCommand::SendPrompt` without `delivery` means queue |
| Send now | `Delivery::SendNow` | cancels the running turn, then sends this prompt |
| Steer | `Delivery::Steer` | the host steers where the agent can, else sends now; the result arrives as the entry's `steer`. No view entry says whether the agent steers natively (that is in the log's `agent_session_attached` capabilities) |
| Delivery from a newer client | `Delivery::Unknown` | never run; refused with `ErrorCode::CommandInvalid` |
| Queued prompts, in order | `ViewEntry::UserMessage` per queued turn | content; actions `HostCommand::EditQueuedPrompt`, `HostCommand::RemoveQueuedPrompt`, `HostCommand::SendQueuedNow` |
| Queue held after a stop other than a normal finish | the last step's stop is not `StopReason::EndTurn` and entries with `UserMessageStatus::Queued` remain | the stop reason; `HostCommand::ResumeQueue` |
| Queue full | `ErrorCode::ThreadQueueFull` | retryable: the prompt can be sent once a queued turn starts |
| Edit, remove or send now refused: the turn already started | `ErrorCode::ThreadTurnNotQueued` | the turn's current state |
| Close to the limit (at least 95% used, or the window's warning) | `AgentLimitsView.meter` (`LimitMeter`, tone `MeterTone::Near`) | tone, used percent, window label, reset time; section 8 |
| Context usage | `HeaderEntry.usage` (`ContextUsage`) | context used, context size, cost |
| Cost line, pay as you go only (doc 30 §5.6) | `AgentLimitsView.billing` is `BillingKind::PayAsYouGo`; `ContextUsage.cost` | amount and currency, an estimate; hidden for `BillingKind::Plan` |

## 6. Versions

The matter's stream: one `ViewEntry::MatterHeader` and one `ViewEntry::Version` per checkpoint, across
every thread in the matter.

Every `ViewEntry::Version` has its checkpoint, kind and time (`ts`), plus thread and turn when the
checkpoint was recorded with them. Only `OutsideEdits` versions carry `files`; a turn's files are on
its step card, and compare (below) lists the files between any two versions.

| State | Type and variant | Data the screen gets |
|---|---|---|
| Import of the folder | `ViewEntry::Version` with `CheckpointKind::Import` | time |
| Before an agent's turn | `CheckpointKind::BeforeTurn` | time, thread and turn (an agent in a thread) |
| After an agent's turn | `CheckpointKind::AfterTurn` | time, thread and turn |
| Edits made outside Clauseo | `CheckpointKind::OutsideEdits` | time, `files` (the lawyer, outside Clauseo) |
| Undo of a step | `CheckpointKind::Undo` | time, thread and turn when recorded; restored and kept paths are on the undo marker (`MarkerKind::StepUndone`) |
| Restore of a version | `CheckpointKind::Restore` | time; the version restored from and the scope are on the restore marker (`MarkerKind::VersionRestored`) |
| Checkpoint of a kind from a newer Clauseo | `CheckpointKind::Unknown` | time only |
| Compare two versions | `HostQuery::CompareVersions` returns `QueryResult::FileChanges` | per file: change kind, sizes, may include the lawyer's edits |
| Restore the whole matter | `RestoreScope::Matter` | `HostCommand::RestoreVersion` |
| Restore some files | `RestoreScope::Files` | the chosen paths |
| Restore refused: a file is open in another program | `ErrorCode::MatterFileLocked` | retryable |
| Restore refused: a turn runs in the matter | `ErrorCode::MatterTurnRunning` | retryable |
| Folder missing (the locate-folder flow, doc 23 §2.2) | `ErrorCode::MatterFolderMissing` | the matter's name |
| Checkpoint failed | `ErrorCode::MatterCheckpointFailed` | the turn is refused before it starts (QR5.3) |

## 7. Matter

| State | Type and variant | Data the screen gets |
|---|---|---|
| Scratch matter, created on first start | `ViewEntry::MatterHeader` with `MatterKind::Scratch` | name |
| New folder | `MatterKind::Folder` from `MatterSource::NewFolder` | name, import summary |
| Existing folder | `MatterKind::Folder` from `MatterSource::ExistingFolder` | name, import summary: files, bytes, cloud-only files |
| Folder refused (drive root, home or system folder) | `ErrorCode::MatterFolderRefused` | choose another folder |
| Matter list | `HostQuery::ListMatters` returns `QueryResult::Matters` | name, kind per matter |
| Thread list | `HostQuery::ListThreads` returns `QueryResult::Threads` | title, agent, status per thread |

## 8. Account limits

Delivered by `HostEvent::AccountLimitsChanged`, never logged. The mapping from `AccountLimits` to
`LimitsView` (freshness needs the clock) is M2's `limits` crate; these are the types it fills. Doc 30
writes the tones in camel case; the wire uses snake case (`at_limit`).

| State | Type and variant | Data the screen gets |
|---|---|---|
| Meter neutral, under 75% | `MeterTone::Neutral` | used percent, window label, reset time |
| Meter near, from 75% or on the window's warning | `MeterTone::Near` | as above |
| Meter at the limit, 100% or rejected | `MeterTone::AtLimit` | as above; never shown as an error |
| Fresh, read under 15 minutes ago | `LimitSource::Fresh` | numbers, reset times, `Freshness::Fresh` |
| Stale, aged or the last read failed | `LimitSource::Stale` with `StaleReason::Aged`, `StaleReason::ReadFailed` | same numbers, muted, with the check time; `Freshness::Stale` |
| Loading, first read | `LimitSource::Loading` | no numbers; `Freshness::Loading` |
| Unavailable, never read or read failed | `LimitSource::Unavailable` with `UnavailableReason::NeverRead`, `UnavailableReason::ReadFailed` | a try-again action; chat never blocked; `Freshness::Unavailable` |
| Unavailable, the sign-in lacks the profile scope | `UnavailableReason::MissingScope` | a sign-in-again action; `Freshness::MissingScope` |
| Not applicable (API key, Bedrock, Vertex) | `LimitSource::NotApplicable` | `BillingKind::PayAsYouGo`, `at_limit_text`, the cost line; no bars; `Freshness::NotApplicable`. The provider (`BillingProvider`) is only in `AccountLimits`, not in `AgentLimitsView` |
| Live only, from the agent's signal while it works | `LimitSource::LiveOnly` | live bars, the update time (`checked_at`); `Freshness::LiveOnly` |
| Freshness from a newer Clauseo | `LimitSource::Unknown` | as unavailable |
| Reset time passed, one row | `WindowFreshness::ResetPassed` | the reset time, refreshing; never the old percentage |
| At the limit, the agent pauses | `AtLimitBehaviour::Pauses` | `at_limit_text`, which M2 `limits` writes from the behaviour |
| At the limit, extra usage | `AtLimitBehaviour::ExtraUsage` | as above (whether extra usage is on is folded into the text) |
| At the limit, credits | `AtLimitBehaviour::Credits` | as above (whether the plan has credits is folded into the text) |
| At-limit card, reset time known | `ViewEntry::LimitStop` with `resets_at` | turn, window, reset time, `LimitStopReason::PlanWindow`; partial output and the checkpoint stay |
| At-limit card, reset time unknown | `ViewEntry::LimitStop` without `resets_at` | as above |
| At-limit card, workspace reasons | `LimitStopReason::WorkspaceCredits`, `LimitStopReason::WorkspaceSpendCap`, `LimitStopReason::Temporary` | as above |
| At-limit card actions: continue at the reset (sandboxed matter only), notify, ask the other signed-in agent | planned: M2 `limits` | reset time, the other agent's sign-in state |
| Sidebar paused | `ThreadStatus::PausedAtLimit` | reset time |
| Assistant-picker line | `AgentLimitsView.meter` per `AgentKind::Claude`, `AgentKind::Codex` | tightest window: used percent, label |
| Plan and usage settings page | `LimitsView` | per agent: plan, `BillingKind::Plan` or `BillingKind::PayAsYouGo`, each `LimitWindowView` (label, used percent, tone, reset time, freshness), at-limit text, check time |

## 9. Errors

Copy is chosen by code; `HostError.message` is for logs and never shown. Retryable codes may be
retried as they are; the others need the lawyer or a fix first. Meanings are in 0003.

| Group | Codes | Retryable |
|---|---|---|
| Internal | `ErrorCode::Internal` | no |
| Command | `ErrorCode::CommandInvalid`, `ErrorCode::CommandConflict`, `ErrorCode::CommandUnknownMatter`, `ErrorCode::CommandUnknownThread` | no |
| Thread | `ErrorCode::ThreadTurnRunning`, `ErrorCode::ThreadQueueFull` | yes |
| Thread | `ErrorCode::ThreadInterrupted`, `ErrorCode::ThreadTurnNotQueued` | no |
| Permission | `ErrorCode::PermissionNotPending`, `ErrorCode::PermissionUnknownOption` | no |
| Elicitation | `ErrorCode::ElicitationNotPending`, `ErrorCode::ElicitationInvalidValues` | no |
| Agent | `ErrorCode::AgentLaunchFailed`, `ErrorCode::AgentExited` | yes |
| Agent | `ErrorCode::AgentProtocolError`, `ErrorCode::AgentAuthRequired`, `ErrorCode::AgentUnsupportedVersion` | no |
| Store | `ErrorCode::StoreIo` | yes |
| Store | `ErrorCode::StoreDiskFull`, `ErrorCode::StoreCorrupt`, `ErrorCode::StoreLocked`, `ErrorCode::StoreNewerVersion` | no |
| Matter | `ErrorCode::MatterCheckpointFailed`, `ErrorCode::MatterFileLocked`, `ErrorCode::MatterTurnRunning` | yes |
| Matter | `ErrorCode::MatterFolderMissing`, `ErrorCode::MatterFolderRefused` | no |
| Transport | `ErrorCode::TransportUnauthorized`, `ErrorCode::TransportTooLarge` | no |
| A code from a newer Clauseo | `ErrorCode::Unknown` | no |

A failed turn shows its code from `turn_failed` on the step card (section 3).

## 10. Planned, not in M1's protocol

These arrive in M2 as additive events and commands.

| State | Type and variant | Data the screen gets |
|---|---|---|
| Onboarding: welcome | planned: M2 `agents` | runtime download started in the background |
| Onboarding: choose the agent | planned: M2 `agents` | the two agents, the API key or firm account path |
| Onboarding: sign in, waiting, cancelled, timed out, no plan, network failure | planned: M2 `agents` | sign-in progress and its failure |
| Onboarding: safe workspace, sandbox unavailable | planned: M2 `sandbox` | tier; the ask-before-each-change fallback |
| Onboarding: first matter, folder unreadable | planned: M2 `agents` (flow); `ErrorCode::MatterFolderRefused` exists | folder state |
| Onboarding: first question, sign-in expired | planned: M2 `agents`; `ErrorCode::AgentAuthRequired` exists | example prompts from the matter's files |
| Protection status: protected | planned: M2 `sandbox` | tier, what the agent can reach |
| Protection status: Not protected | planned: M2 `sandbox` | tier C; persistent on the matter |
| One-time explanation of Not protected | planned: M2 `sandbox` | shown once per machine |
| Denial card: a blocked web domain | planned: M2 `sandbox` | the domain; allowing it |
| Denial card: a blocked folder | planned: M2 `sandbox` | the path; allowing it restarts the agent and resumes the thread |
| Agent download progress | planned: M2 `agents` | percent, failure |
| Resuming after an idle stop | planned: M2 `agents` | the thread; `ThreadStatus::IdleStopped` is section 1 |
