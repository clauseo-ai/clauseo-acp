# UI states

Every state the screens must design for, and where its data comes from. Each row names the state, the
type and variant that produce it (or the crate that will add it), and the data the screen gets.
This file holds engineering facts only; design writes the copy. The test
`every_listed_variant_is_in_ui_states` in `crates/protocol/tests/ui_states.rs` checks that every
variant of each enum it lists appears in sections 1 to 9, as `Type::Variant` in the second column of
a table row. Prose, code blocks, HTML comments, indented code and section 10 do not count.

A variant named `Unknown` is a value from a newer Clauseo that this version does not know. It is
never produced by this version, so each needs only a neutral fallback.

Paths (`MatterPath`) may hold bidi marks such as U+200F, so every path is shown bidi-isolated
(`<bdi>` or `unicode-bidi: isolate`).

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
| Agent stopped while idle (the `agents` crate will emit it) | `ThreadStatus::IdleStopped` | the status only (logged as `DetachReason::IdleStopped`); resuming is in section 10 |
| Archived | `ThreadStatus::Archived` | title, agent; read-only |

Detach reasons are in the log only (`agent_session_detached`, read through `SubscribeEvents`); no view
entry carries them: `DetachReason::Exited`, `DetachReason::Closed`, `DetachReason::Restarting`,
`DetachReason::IdleStopped`, and `DetachReason::Unknown`.

## 2. Transcript entries

Entries sort by `order`. Text streams through `ViewPatchBody::AppendText`; everything else arrives as
`ViewPatchBody::Upsert`. Opening a thread, or a lost cursor, sends `ViewPatchBody::Reset` with a
`ViewWindow`: the header, about 200 recent entries, and `has_older` when older entries exist. Older
entries page in on scroll through `HostQuery::PageEntries`, which returns `QueryResult::Entries`.

| State | Type and variant | Data the screen gets |
|---|---|---|
| User message, sent | `ViewEntry::UserMessage` with `UserMessageStatus::Sent`, `Delivery::Queue` | content blocks (text, attached files), turn, delivery; a long text block arrives cut and grows by `AppendText` with `AppendTarget::UserBlock` and its index |
| User message, queued behind a running turn | `UserMessageStatus::Queued` | content, turn; edit, remove and send now in section 5 |
| User message, edited while queued | `UserMessageStatus::Queued` with `edited` | the new content; same turn, same place |
| User message, removed from the queue or cancelled before it started | `UserMessageStatus::Removed` | the text stays, so it can be sent again |
| User message, sent now | `Delivery::SendNow` | content; the running turn ends `StopReason::Cancelled` |
| User message, steered | `Delivery::Steer` | content; then the entry's `steer` holds one outcome below |
| Steer taken into the running turn | `SteerOutcome::Injected` | the turn that answers it |
| Steer moved to the next turn (the turn had just ended) | `SteerOutcome::Requeued` | the new turn, which starts next |
| Steer sent as send now (no real steer for this agent) | `SteerOutcome::SentNow` | the new turn; the running one is cancelled |
| Steer outcome from a newer Clauseo | `SteerOutcome::Unknown` | the outcome's kind only |
| Answer, streaming | `ViewEntry::AgentText`, `TextKind::Answer` | text so far, growing by `AppendText` |
| Answer, complete | `ViewEntry::AgentText` | full text; the turn has ended |
| Thought, streaming and complete | `ViewEntry::AgentText`, `TextKind::Thought` | as the answer |
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
| Tool call carried on in the background while a steer was answered | `ToolCallEntry.background` (`BackgroundMark`) | the steered message, the finish time once it finished |
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
| Notice from the agent | `ViewEntry::Notice` with no `kind` | text, the agent's own words |
| Host notice: the agent no longer remembers the earlier turns | `NoticeKind::ContextNotRestored` | none; the app's own words (the host's English `text` until it renders the kind) |
| Host notice: papers an undo removed came back (a sync client put them back) and stay | `NoticeKind::FilesCameBack` | files (at most five, sorted by path), more (how many beyond them); the app's own words (the host's English `text`, a count only, until it renders the kind) |
| Host notice of a kind from a newer Clauseo | `NoticeKind::Unknown` | text, the host's English fallback |
| Host notice: a skill pack the launch left out | `NoticeKind::SkillPackNotLoaded`, with `pack` (the pack's id) and `reason` (`PackSkipReason`) | the pack and the reason in the app's own words (the host's English `text` until the app renders the kind) |
| Compaction started and completed | `ViewEntry::Marker` with `MarkerKind::Compaction`: `CompactionStatus::Started`, `CompactionStatus::Completed` | summary (inline or blob) when completed |
| Undo marker | `ViewEntry::Marker` with `MarkerKind::StepUndone` | the step's turn, the new checkpoint, restored paths, kept paths |
| Restore marker | `ViewEntry::Marker` with `MarkerKind::VersionRestored` | the version restored from, the new checkpoint, scope |
| Located folder marker ("Found at ...") | `ViewEntry::Marker` with `MarkerKind::MatterRelocated` | the folder's path |

## 3. Step card

One `ViewEntry::Step` per ended turn that started, and per turn that failed before it started (its
agent's launch failed; decision 0002) (a turn cancelled or removed before it starts gets none;
see the cancelled row): stop reason, the failure code (`failure`) when it failed, files
changed, skipped files, the after-turn checkpoint or none, `undone` once the step was undone, and
token usage (`usage`: total, input, output, thought, `cached_read`, `cached_write`) when the agent
reported it.

| State | Type and variant | Data the screen gets |
|---|---|---|
| Finished normally | `StopReason::EndTurn` | files changed; the queue moves on |
| Stopped at the token limit | `StopReason::MaxTokens` | files changed; the queue is held |
| Stopped at the agent's turn-request limit | `StopReason::MaxTurnRequests` | as above |
| Agent refused | `StopReason::Refusal` | as above |
| Cancelled (by the lawyer or by send now) | `StopReason::Cancelled` | as above; a queued prompt removed before it started gets no step card, only `UserMessageStatus::Removed` |
| Ended by a refusal: the agent cancelled the turn right after the lawyer's Don't allow (Codex), with no Stop after it | `StopReason::NotAllowed` | as cancelled; the words say the lawyer did not allow it and the answer ended there |
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
| Checkpoint missing, undo impossible | no `checkpoint_after`, so the UI derives undo as not possible | shown, never silent |
| Step already undone | `undone` true | the undo marker (section 2) |
| Undo disabled while a turn runs in this thread | `HeaderEntry.status` is `ThreadStatus::Starting`, `ThreadStatus::Running`, `ThreadStatus::NeedsYou` or `ThreadStatus::Queued` (the derived undo stays as it is) | the step |
| Undo refused: a turn runs in another thread of the matter | the host checks when the command arrives: `ErrorCode::MatterTurnRunning` | the code only (retryable) |
| Undo with a choice per file | `HostCommand::UndoStep` with `FileChoice::Undo`, `FileChoice::Keep` | the step's files |
| Undo asks first: a file changed after Clauseo's edit | `HostQuery::PreviewUndo` returns `QueryResult::UndoPreview` | `edited_since`: the step's files whose content now differs from its after-turn checkpoint; empty means Undo needs no ask |

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
| Permission refused because a program holds the file open (`held_open`): "Word has Brief.docx open. Close it in Word or save a copy, then ask again." | `PermissionState::Resolved` with `held_open` set; the choice is the reject-once option, or `PermissionChoice::Cancelled` when none was offered | the matter path a program holds open (Word, Excel or PowerPoint by an owner file, or any program on Windows); the lawyer was not asked |
| Permission for the agent's own plan note, in its own config folder (not one of the lawyer's files) | `PermissionAbout::AgentPlan` in the entry's `about` | as a pending permission; asking and answering are unchanged |
| Permission about something from a newer Clauseo | `PermissionAbout::Unknown` | as a permission without `about` |
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
| Elicitation ended in a way from a newer Clauseo | `ElicitationOutcome::Unknown` | answered, by whom; no outcome named |
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
| Steer | `Delivery::Steer` | the host steers where the agent can, else sends now; the result arrives as the entry's `steer`. Before sending, `HeaderEntry.steer_joins_turn` says which: true when a steer adds to the running answer (claude-agent-acp), false when it cancels and sends now (the answer so far is kept) |
| Delivery from a newer client | `Delivery::Unknown` | never run: `HostCommand::SendPrompt` takes the closed `DeliveryRequest`, so the command fails to decode with `ErrorCode::CommandInvalid` |
| Queued prompts, in order | `ViewEntry::UserMessage` per queued turn | content; actions `HostCommand::EditQueuedPrompt` (new text; the files stay), `HostCommand::RemoveQueuedPrompt`, `HostCommand::SendQueuedNow`; at most `MAX_QUEUED_TURNS` |
| Queue held, also after a restart | `HeaderEntry.queue_held` in the thread header, `ThreadSummary.queue_held` in the sidebar | the entries with `UserMessageStatus::Queued` remain, with `HostCommand::ResumeQueue`; the last step's stop reason (section 3) only when the thread has one |
| Queue full | `ErrorCode::ThreadQueueFull` | retryable: the prompt can be sent once a queued turn starts |
| Edit, remove or send now refused: the turn already started | `ErrorCode::ThreadTurnNotQueued` | the turn's current state |
| Close to the limit: `used_percent` at 95% or more, or the window's `warning` | `AgentLimitsView.meter` (`LimitMeter`, tone `MeterTone::Near`) | tone (`MeterTone::Near` starts at 75%, not at this 95%), used percent, window label, reset time; section 8 |
| Context usage | `HeaderEntry.usage` (`ContextUsage`) | context used, context size, cost |
| Cost line, pay as you go only | `AgentLimitsView.billing` is `BillingKind::PayAsYouGo`; `ContextUsage.cost` | amount and currency, an estimate; hidden for `BillingKind::Plan` |

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
| Compare two versions | `HostQuery::CompareVersions` with `to` a checkpoint returns `QueryResult::FileChanges` | per file: change kind, sizes, may include the lawyer's edits |
| Compare a version with the folder now | `HostQuery::CompareVersions` with `to` missing or `null` returns `QueryResult::FileChanges` | the same per file fields; nothing is stored |
| Preview a restore | `HostQuery::PreviewRestore` returns `QueryResult::RestorePreview` | `changes` (files written), `added_since` (left in place), `kept` (left as they are), `locked` (open in another program) |
| Restore the whole matter | `RestoreRequest::Matter` | `HostCommand::RestoreVersion` |
| Restore some files | `RestoreRequest::Files` | the chosen paths |
| Restore or undo refused: a file is open in another program | `ErrorCode::MatterFileLocked` | retryable; `HostError.files`, the files to close |
| Restore refused: a turn runs in the matter | `ErrorCode::MatterTurnRunning` | retryable |
| Folder missing (the locate-folder flow) | `ErrorCode::MatterFolderMissing` | the matter's name |
| Checkpoint failed | `ErrorCode::MatterCheckpointFailed` | the turn is refused before it starts |

## 7. Matter

| State | Type and variant | Data the screen gets |
|---|---|---|
| Scratch matter, created on first start | `ViewEntry::MatterHeader` with `MatterKind::Scratch` | name |
| New folder | `MatterKind::Folder` from `MatterSource::NewFolder` | name, import summary |
| Existing folder | `MatterKind::Folder` from `MatterSource::ExistingFolder` | name, import summary: files, bytes, cloud-only files |
| New folder in the default location, no picker | `MatterKind::Folder` from `MatterSource::DefaultLocation` | name |
| Where new matters go, before a pick | `HostQuery::GetDefaultLocation` returns `QueryResult::DefaultLocation` | `display`, the folder below home |
| Picked folder's support for Codex's sandbox (Windows), before the matter exists | `HostQuery::FolderSupport { source }` (the `CreateMatter` source) returns `QueryResult::FolderSupport`; `check_failed` when the check doesn't answer in 5 s; an existing folder create refuses gets create's `matter.folder_*` code | `support`: `network`, `no_permissions`, `cant_change_permissions`, `check_failed`, or missing |
| Folder refused, one code per reason (`matter.folder_refused` before decision record 0022) | `ErrorCode::MatterFolderIsDrive`, `ErrorCode::MatterFolderIsHome`, `ErrorCode::MatterFolderIsSystem`, `ErrorCode::MatterFolderUnreadable`, `ErrorCode::MatterFolderInMattersRoot` | choose another folder |
| Folder is already a matter's, or nested with one | `ErrorCode::MatterFolderIsMatter` | `HostError.matter`, the matter to name and offer to open |
| Matter list | `HostQuery::ListMatters` returns `QueryResult::Matters` | name, kind, client, next date, pinned, last opened, last activity, folder display, status and protection per matter |
| Matter protected (0051) | `Protection::Protected` with `ProtectionTier::A` or `ProtectionTier::B` | `tier`; `MatterSummary::protection`, `HostEvent::ProtectionChanged` |
| Matter Not protected (0051) | `Protection::NotProtected` (tier `ProtectionTier::C`) | `cause`, below; missing `protection` means not known yet |
| Not protected: no sandbox on this computer | `NotProtectedCause::Unsupported` | Linux, Windows on ARM, a build without it |
| Not protected: set-up not done | `NotProtectedCause::NeedsSetUp` | offer the set-up (`HostCommand::RetryProtection`) |
| Not protected: set-up refused | `NotProtectedCause::SetUpRefused` | the administrator prompt was declined; offer it again |
| Not protected: set-up failed | `NotProtectedCause::SetUpFailed` | offer to try again |
| Not protected: the pool is full | `NotProtectedCause::PoolFull` | counts: `size`, `open`, `held`, `owed`, `set_aside` |
| Not protected: switched off | `NotProtectedCause::SwitchedOff` | `at` |
| Not protected: a cause from a newer Clauseo | `NotProtectedCause::Unknown` | show as Not protected |
| This computer's protection (0051) | `HostQuery::Protection` returns `QueryResult::Protection` | `tier`, `computer` (a `Protection`), `required`, `unprotected_auto_allowed`; `command.invalid` until the host builds it |
| Remove the sandbox's set-up (0051) | `HostCommand::RemoveSandboxSetup` | one administrator prompt; `command.invalid` until the host builds it |
| Matter idle | `MatterStatus::Idle` | nothing runs, nothing to read |
| Matter working | `MatterStatus::Working` | a thread's turn starts, runs or waits for the writer |
| Matter needs a permission | `MatterStatus::NeedsPermission` | outranks every other status |
| Matter needs an answer | `MatterStatus::NeedsAnswer` | an elicitation waits |
| Matter answer ready | `MatterStatus::Ready` | a turn finished after the matter was last opened |
| Matter stopped | `MatterStatus::Stopped` | `at`, when the turn stopped short |
| Matter paused at the limit | `MatterStatus::PausedAtLimit` | `resets_at`, the earliest reset, when known |
| Matter details changed, opened, forgotten | `HostCommand::UpdateMatter`, `HostCommand::OpenMatter`, `HostCommand::ForgetMatter` | `Done`; forgetting deletes nothing |
| Thread list | `HostQuery::ListThreads` returns `QueryResult::Threads` | title, agent, status per thread |
| Papers (the folder as it is now; asked again after a `checkpoint_recorded` or `outside_edits`) | `HostQuery::ListPapers` returns `QueryResult::Papers` | per file Clauseo versions: `FileRefId`, path, size, modified; sorted by path |
| Files Clauseo does not version | `QueryResult::Papers` `skipped` | path and `SkippedFileReason` (over the size cap, cloud-only) |
| Folders that could not be read (no permission, or not downloaded) | `QueryResult::Papers` `unread` | folder paths; their files are kept in the history as they were |
| More entries than the list holds | `QueryResult::Papers` with `truncated` | the first 2,000 entries by path (`MAX_PAPERS`): papers, skipped files and unread folders together |

## 8. Account limits

Delivered by `HostEvent::AccountLimitsChanged`, never logged. The window's whole limits view
(`LimitsView`) arrives by `HostEvent::LimitsViewChanged` on the limits subscription, never logged and
in no stream. The mapping from `AccountLimits` to `LimitsView` (freshness needs the clock) is
`limits`' job (`crates/limits/src/view.rs`). The wire writes the tones in snake case (`at_limit`).

| State | Type and variant | Data the screen gets |
|---|---|---|
| Meter neutral, under 75% | `MeterTone::Neutral` | used percent, window label, reset time |
| Meter near, from 75% or on the window's warning | `MeterTone::Near` | as above |
| Meter at the limit, 100% or rejected | `MeterTone::AtLimit` | as above; never shown as an error |
| Fresh, read under 15 minutes ago | `LimitSource::Fresh` | numbers, reset times, `Freshness::Fresh` |
| Stale, aged or the last read failed | `LimitSource::Stale` with `StaleReason::Aged`, `StaleReason::ReadFailed` | same numbers, muted, with the check time; `Freshness::Stale` |
| Loading, first read | `LimitSource::Loading` | no numbers; `Freshness::Loading` |
| Unavailable, never read or read failed | `LimitSource::Unavailable` with `UnavailableReason::NeverRead`, `UnavailableReason::ReadFailed` | a try-again action (`HostCommand::RefreshLimits` with `RefreshWhy::TryAgain`); chat never blocked; `Freshness::Unavailable` |
| Unavailable, the sign-in lacks the profile scope | `UnavailableReason::MissingScope` | a sign-in-again action; `Freshness::MissingScope` |
| Not applicable (API key, Bedrock, Vertex) | `LimitSource::NotApplicable` | `BillingKind::PayAsYouGo`, `at_limit_text`, the cost line; no bars; `Freshness::NotApplicable`. `AgentLimitsView.provider` (`BillingProvider`) names who bills it: `BillingProvider::Anthropic` (Anthropic), `BillingProvider::Bedrock` (AWS), `BillingProvider::Vertex` (Google Cloud), `BillingProvider::OpenAi` (OpenAI API account), `BillingProvider::Other`; no provider for a plan |
| Live only, from the agent's signal while it works | `LimitSource::LiveOnly` | live bars, the update time (`checked_at`); `Freshness::LiveOnly` |
| Freshness from a newer Clauseo | `LimitSource::Unknown` | as unavailable |
| Reset time passed, one row | `WindowFreshness::ResetPassed` | the reset time, refreshing; never the old percentage |
| Next refresh on the settings page, planned copy "Refreshes at the 4:45 pm reset" | `AgentLimitsView.next_read_at` | the next shown reset time; left out when no reset is pending |
| Refresh when a window gets focus, or a meter or the settings page opens | `HostCommand::RefreshLimits` with `RefreshWhy::Shown` | reads only data older than 5 minutes; answers `Done` and logs nothing; `agent` left out means every agent |
| Try again | `HostCommand::RefreshLimits` with `RefreshWhy::TryAgain` | reads now unless a read is in flight or the last read started under 10 s ago (then it re-emits the view); no read while that agent has a permission or elicitation pending, it starts once answered |
| At the limit, the agent pauses | `AtLimitBehaviour::Pauses` | `at_limit_text`, which the planned `limits` crate writes from the behaviour |
| At the limit, extra usage | `AtLimitBehaviour::ExtraUsage` | as above (whether extra usage is on is folded into the text) |
| At the limit, credits | `AtLimitBehaviour::Credits` | as above (whether the plan has credits is folded into the text) |
| At-limit card, reset time known | `ViewEntry::LimitStop` with `resets_at` | turn, window, reset time, `LimitStopReason::PlanWindow`; partial output and the checkpoint stay |
| At-limit card, reset time unknown | `ViewEntry::LimitStop` without `resets_at` | as above |
| At-limit card, workspace reasons | `LimitStopReason::WorkspaceCredits`, `LimitStopReason::WorkspaceSpendCap`, `LimitStopReason::Temporary` | as above |
| At-limit card actions: continue at the reset (sandboxed matter only), notify, ask the other signed-in agent | planned: the `limits` crate | reset time, the other agent's sign-in state |
| Sidebar paused | `ThreadStatus::PausedAtLimit` | reset time |
| Agent-picker line | `AgentLimitsView.meter` per `AgentKind::Claude`, `AgentKind::Codex` | tightest window: used percent, label |
| Plan and usage settings page | `LimitsView` | per agent: plan, `BillingKind::Plan` or `BillingKind::PayAsYouGo` with its provider, each `LimitWindowView` (label, used percent, tone, reset time, freshness), at-limit text, check time, next read time |

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
| Claude refused: its guard can't be set up in a forced-ask folder (reinstall) | `ErrorCode::AgentGuardMissing` | no |
| Store | `ErrorCode::StoreIo` | yes |
| Store | `ErrorCode::StoreDiskFull`, `ErrorCode::StoreCorrupt`, `ErrorCode::StoreLocked`, `ErrorCode::StoreNewerVersion` | no |
| Matter | `ErrorCode::MatterCheckpointFailed`, `ErrorCode::MatterFileLocked`, `ErrorCode::MatterTurnRunning` | yes |
| Matter | `ErrorCode::MatterFolderMissing`, `ErrorCode::MatterCodexFolder`; `ErrorCode::MatterFolderRefused` only from a host before decision record 0022 | no |
| Codex refused: its sandbox can't work in the folder (Windows) | `ErrorCode::MatterFolderNetwork`, `ErrorCode::MatterFolderNoPermissions` | no |
| Matter folder refused (0022) | `ErrorCode::MatterFolderIsDrive`, `ErrorCode::MatterFolderIsHome`, `ErrorCode::MatterFolderIsSystem`, `ErrorCode::MatterFolderUnreadable`, `ErrorCode::MatterFolderIsMatter`, `ErrorCode::MatterFolderInMattersRoot`, `ErrorCode::MatterFolderTooDeep` (Windows) | no |
| Transport | `ErrorCode::TransportUnauthorized`, `ErrorCode::TransportTooLarge` | no |
| Settings | `ErrorCode::SettingsLocked`, `ErrorCode::SettingsRefused`, `ErrorCode::SettingsKeyStored` | no |
| Agent install (0048) | `ErrorCode::AgentInstallOffline`, `ErrorCode::AgentInstallDiskFull`, `ErrorCode::AgentInstallFailed` | yes |
| Agent install (0048) | `ErrorCode::AgentInstallBlocked` | no |
| Sign-in (0048) | `ErrorCode::SigninLinkExpired`, `ErrorCode::SigninPortBusy`, `ErrorCode::SigninFailed` | yes |
| Sign-in (0048) | `ErrorCode::SigninCancelled`, `ErrorCode::SigninNoPlan`, `ErrorCode::SigninDeviceCodeOff`, `ErrorCode::SigninPolicyUnreadable`, `ErrorCode::SigninLoginShared`, `ErrorCode::SigninFirmManaged`, `ErrorCode::SigninKeyInUse`, `ErrorCode::SigninEndpointInUse` | no |
| Update (0037) | `ErrorCode::UpdateFailed` | yes |
| Leave order | `ErrorCode::SigninAgentsStillRunning` | yes |
| Launch while the keys load (0048) | `ErrorCode::AgentKeysLoading` | yes |
| Launch with an unreadable key (0048) | `ErrorCode::AgentKeyUnreadable` | yes |
| Open in Word with a path too long for Word (0026, Windows) | `ErrorCode::MatterDocumentPathTooLong` | no |
| A protected name in the matter is a shortcut (Windows, S15 PR4a2) | `ErrorCode::MatterProtectedLink` | no |
| Sandbox set-up refused or failed (0051) | `ErrorCode::SandboxSetupRefused`, `ErrorCode::SandboxSetupFailed` | yes |
| Sandbox set-up needed, or required by the firm and not available (0051) | `ErrorCode::SandboxSetupRequired`, `ErrorCode::SandboxRequired` | no |
| A code from a newer Clauseo | `ErrorCode::Unknown` | no |

A failed turn shows its code from `turn_failed` on the step card (section 3).

## 10. Planned, not in this protocol yet

These arrive later as additive events and commands.

| State | Type and variant | Data the screen gets |
|---|---|---|
| Onboarding: welcome | planned: the `agents` crate | runtime download started in the background |
| Onboarding: choose the agent | planned: the `agents` crate | the two agents, an API key or an account the firm provides |
| Onboarding: sign in, waiting, cancelled, timed out, no plan, network failure | planned: the `agents` crate | sign-in progress and its failure |
| Onboarding: matter protection, sandbox unavailable | planned: the `sandbox` crate | the protection level; the ask-before-each-change fallback |
| Onboarding: first matter, folder unreadable | planned: the `agents` crate (flow); `ErrorCode::MatterFolderUnreadable` exists | folder state |
| Onboarding: first question, sign-in expired | planned: the `agents` crate; `ErrorCode::AgentAuthRequired` exists | example prompts from the matter's files |
| Protection status: protected | planned: the `sandbox` crate | the protection level, what the agent can reach |
| Protection status: Not protected | planned: the `sandbox` crate | the matter runs without the sandbox; persistent on the matter |
| One-time explanation of Not protected | planned: the `sandbox` crate | shown once per machine |
| Denial card: a blocked web domain | planned: the `sandbox` crate | the domain; allowing it |
| Denial card: a blocked folder | planned: the `sandbox` crate | the path; allowing it restarts the agent and resumes the thread |
| Agent download progress | planned: the `agents` crate | percent, failure |
| Resuming after an idle stop | planned: the `agents` crate | the thread; `ThreadStatus::IdleStopped` is section 1 |
