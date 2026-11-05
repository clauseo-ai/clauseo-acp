//! Commands: shape, principal, receipt, state, in that order; one commit per command carrying
//! its receipt; concurrent duplicates sharing the first answer.
//!
//! A command waits in [`Actor::commands`] from its receipt lookup until its answer, so a
//! duplicate that arrives meanwhile joins it (same hash) or is refused (`command.conflict`).
//! A refused command stores no receipt. A failed commit answers with the stream's first
//! failure.

mod to_turn;

use clauseo_protocol::{
    CommandEnvelope, CommandId, CommandOutcome, DomainEvent, ErrorCode, HostCommand, MatterId,
    Principal, StreamId, ThreadArchived, ThreadCreated, ThreadId, ThreadRenamed, Timestamp, Title,
    ValidatedCommand,
};
use clauseo_store::{Receipt, ThreadRow, command_hash};

use super::input::Reply;
use super::matters::{PendingMatter, Stage};
use super::settings::{
    first_act, set_privacy_settings, set_skill_pack, set_tool_settings, set_ui_settings,
};
use super::{Actor, MatterEntry, Tag, ThreadEntry, title_source, triage, turn};
use crate::error::Error;
use crate::pipeline::{Draft, Failure};
use crate::principal::{Access, authorize};

/// A command between its receipt lookup and its answer.
pub(crate) struct PendingCommand {
    pub(super) hash: [u8; 32],
    /// Everyone waiting for the answer: the first caller and its duplicates.
    waiters: Vec<Reply<CommandOutcome>>,
    /// The command, until it runs.
    command: Option<ValidatedCommand>,
    /// A matter being created or imported for it.
    pub(super) matter: Option<Box<PendingMatter>>,
}

/// What a command's code needs to build its receipt and answer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CommandCtx {
    pub(crate) id: CommandId,
    pub(crate) hash: [u8; 32],
}

impl CommandCtx {
    /// The receipt the command's commit carries.
    pub(crate) fn receipt(
        &self,
        stream: Option<StreamId>,
        result: CommandOutcome,
        ts: Timestamp,
    ) -> Receipt {
        Receipt {
            command_id: self.id,
            stream,
            payload_hash: self.hash,
            result,
            ts,
        }
    }
}

/// What a command's commit does before the command is answered.
pub(crate) enum AfterCommand {
    Nothing,
    /// A new matter: known from now on.
    Matter(Box<MatterEntry>),
    /// A new thread: known from now on.
    Thread(Box<ThreadEntry>),
    /// A thread row changed when the commit was sent; put `before` back if it fails.
    Row {
        before: Box<ThreadRow>,
    },
    /// A relocated matter: its row names `folder` once durable.
    Relocated {
        matter: MatterId,
        folder: String,
    },
    /// A turn command's continuation.
    Turn(turn::TurnTag),
    /// The commit logging a restore of this matter.
    Restore(MatterId),
    /// A matter row changed when the commit was sent; put `before` back if it fails.
    MatterRow {
        before: Box<clauseo_store::MatterRow>,
    },
    /// `ForgetMatter` of this matter, already moved to `Actor::forgotten`.
    Forget(MatterId),
    /// A forgotten matter coming back with this row: on the list once durable.
    Unforget(Box<clauseo_store::MatterRow>),
}

/// `dispatch`: shape, principal, then the receipt lookup (or joining a duplicate).
pub(super) fn dispatch(actor: &mut Actor, envelope: CommandEnvelope, reply: Reply<CommandOutcome>) {
    let checked = envelope
        .validate()
        .map_err(Error::Invalid)
        .and_then(|command| {
            authorize(&Principal::LocalDesktop, Access::Dispatch).map(|()| command)
        });
    let command = match checked {
        Ok(command) => command,
        Err(error) => {
            let _gone = reply.send(Err(error));
            return;
        }
    };
    let id = command.command_id();
    let hash = command_hash(command.command());
    if let Some(pending) = actor.commands.get_mut(&id) {
        if pending.hash == hash {
            pending.waiters.push(reply);
        } else {
            let _gone = reply.send(Err(conflict(id)));
        }
        return;
    }
    actor.commands.insert(
        id,
        PendingCommand {
            hash,
            waiters: vec![reply],
            command: Some(command),
            matter: None,
        },
    );
    actor.work.receipts += 1;
    let store = actor.store.clone();
    actor.spawn_post(async move {
        let result = store.receipt(id).await;
        super::input::Input::Receipt {
            command: id,
            result,
        }
    });
}

/// The receipt lookup came back: the stored answer, a conflict, or run the command.
pub(super) fn on_receipt(
    actor: &mut Actor,
    id: CommandId,
    result: Result<Option<Receipt>, clauseo_store::Error>,
) {
    actor.work.receipts = actor.work.receipts.saturating_sub(1);
    let Some(pending) = actor.commands.get_mut(&id) else {
        return;
    };
    let hash = pending.hash;
    match result {
        Ok(Some(receipt)) if receipt.payload_hash == hash => {
            actor.finish_command(id, Ok(receipt.result));
        }
        Ok(Some(_)) => actor.finish_command(id, Err(conflict(id))),
        Err(error) => actor.finish_command(id, Err(Error::Store(error))),
        Ok(None) => {
            let Some(command) = pending.command.take() else {
                return;
            };
            if !matches!(actor.phase, super::HostPhase::Serving) {
                actor.finish_command(id, Err(Error::Closed));
                return;
            }
            let ctx = CommandCtx { id, hash };
            run_command(actor, &ctx, command.into_envelope().command);
        }
    }
}

/// Answers every caller waiting on command `id` and forgets it. Duplicates get the same
/// answer: an error is passed on with its code and message.
pub(crate) fn finish_command(
    actor: &mut Actor,
    id: CommandId,
    result: Result<CommandOutcome, Error>,
) {
    actor.claims.remove(&id);
    let Some(pending) = actor.commands.remove(&id) else {
        return;
    };
    // Matter rows change only by commands and by a `ListMatters` folder read; the app lists
    // compare every row at the next flush.
    actor.lists.touch_all();
    let mut waiters = pending.waiters.into_iter();
    let Some(first) = waiters.next() else {
        return;
    };
    // A refusal keeps all it carries, `HostError::matter` included, and a matter's
    // `file_locked` its files.
    let shared = result
        .as_ref()
        .map(|outcome| *outcome)
        .map_err(Error::to_wire);
    let _gone = first.send(result);
    for waiter in waiters {
        let _gone = waiter.send(shared.clone().map_err(Error::Refused));
    }
}

/// The state checks and the work of one command.
fn run_command(actor: &mut Actor, ctx: &CommandCtx, command: HostCommand) {
    match command {
        HostCommand::CreateMatter {
            name,
            source,
            client,
            next_date,
        } => {
            super::matters::create_matter(actor, ctx, &name, source, (client, next_date));
        }
        HostCommand::UpdateMatter {
            matter,
            client,
            next_date,
            skill_packs,
            pinned,
        } => {
            let update = super::matter_details::Update {
                client,
                next_date,
                skill_packs,
                pinned,
            };
            super::matter_details::update_matter(actor, ctx, matter, update);
        }
        HostCommand::OpenMatter { matter } => {
            super::matter_details::open_matter(actor, ctx, matter);
        }
        HostCommand::ForgetMatter { matter } => {
            super::matter_details::forget_matter(actor, ctx, matter);
        }
        HostCommand::RelocateMatter { matter, folder } => {
            super::relocate::relocate_matter(actor, ctx, matter, folder);
        }
        HostCommand::MarkSeen { thread } => super::seen::mark_seen(actor, ctx, thread),
        HostCommand::SnoozeThread { thread, until } => triage::snooze(actor, ctx, thread, until),
        HostCommand::MarkThreadDone { thread, done } => triage::mark_done(actor, ctx, thread, done),
        HostCommand::SweepDone { day_start } => triage::sweep_done(actor, ctx, day_start),
        HostCommand::CreateThread { matter, agent } => {
            if let Err(error) = create_thread(actor, ctx, matter, agent) {
                actor.finish_command(ctx.id, Err(error));
            }
        }
        HostCommand::RenameThread { .. } => title_source::rename_thread(actor, ctx, command),
        HostCommand::ArchiveThread { thread } => {
            thread_command(actor, ctx, thread, ThreadChange::Archive);
        }
        command @ (HostCommand::SendPrompt { .. }
        | HostCommand::Cancel { .. }
        | HostCommand::AnswerPermission { .. }
        | HostCommand::AnswerElicitation { .. }
        | HostCommand::SetConfigOption { .. }
        | HostCommand::SetMode { .. }
        | HostCommand::UndoStep { .. }
        | HostCommand::EditQueuedPrompt { .. }
        | HostCommand::RemoveQueuedPrompt { .. }
        | HostCommand::SendQueuedNow { .. }
        | HostCommand::ResumeQueue { .. }) => to_turn::run_turn_command(actor, ctx, command),
        HostCommand::RestoreVersion {
            matter,
            checkpoint,
            scope,
        } => turn::restore_version(actor, *ctx, matter, checkpoint, scope),
        HostCommand::SetUiSettings { patch } => set_ui_settings(actor, ctx, &patch),
        HostCommand::SetToolSettings { patch } => set_tool_settings(actor, ctx, &patch),
        HostCommand::SetSkillPack { id, on } => set_skill_pack(actor, ctx, &id, on),
        HostCommand::ScanFolder { folder } => super::scan::scan_folder(actor, ctx, folder),
        HostCommand::CancelScan { folder } => super::scan::cancel_scan(actor, ctx, folder),
        HostCommand::SetPrivacySettings { patch } => set_privacy_settings(actor, ctx, &patch),
        HostCommand::FirstAct { counts } => first_act(actor, ctx, counts),
        // No receipt and no event: a repeated id asks the model again, under its read rules.
        HostCommand::RefreshLimits { agent, why } => {
            super::limits::refresh(actor, agent, why);
            actor.finish_command(ctx.id, Ok(CommandOutcome::Done));
        }
        HostCommand::RestartAgent { .. } => not_built(actor, ctx),
        HostCommand::InstallBrowser {} => super::browser_install::install(actor, ctx),
        HostCommand::CancelBrowserInstall {} => super::browser_install::cancel(actor, ctx),
        HostCommand::CheckForUpdate {}
        | HostCommand::InstallUpdate { .. }
        | HostCommand::CancelScheduledUpdate {}
        | HostCommand::SetUpdateMode { .. } => super::update::not_wired(actor, ctx),
    }
}

/// A command the host does not run yet (`RestartAgent`): `command.invalid`.
fn not_built(actor: &mut Actor, ctx: &CommandCtx) {
    let refusal = Error::refused(ErrorCode::CommandInvalid, "not built yet");
    actor.finish_command(ctx.id, Err(refusal));
}

/// A command's commit returned.
pub(super) fn command_committed(
    actor: &mut Actor,
    id: CommandId,
    answer: CommandOutcome,
    after: AfterCommand,
    outcome: Result<(), &Failure>,
) {
    super::counting::committed(actor, id, outcome.is_ok());
    match (after, outcome) {
        (AfterCommand::Matter(entry), Ok(())) => super::matters::recorded(actor, *entry),
        (AfterCommand::Matter(entry), Err(failure)) => {
            let error = Error::Store(failure.error.clone());
            // Answered once the store is asked for the row (`matters/made.rs`).
            let matter = entry.row.id;
            return super::matters::discard(actor, Some(id), matter, error, Stage::Store);
        }
        (AfterCommand::Thread(entry), Ok(())) => {
            actor.threads.insert(entry.row.id, *entry);
        }
        (AfterCommand::Row { before }, Err(_)) => {
            if let Some(entry) = actor.threads.get_mut(&before.id) {
                entry.row = *before;
            }
        }
        (AfterCommand::Relocated { matter, folder }, outcome) => {
            super::relocate::committed(actor, matter, folder, outcome.is_ok());
        }
        (AfterCommand::MatterRow { before }, Err(_)) => {
            super::matter_details::row_failed(actor, *before);
        }
        (AfterCommand::Forget(matter), outcome) => {
            super::matter_details::forget_committed(actor, matter, outcome.is_ok());
        }
        (AfterCommand::Unforget(row), outcome) => {
            super::matter_details::unforget_committed(actor, *row, outcome.is_ok());
        }
        (
            AfterCommand::Nothing
            | AfterCommand::Thread(_)
            | AfterCommand::Row { .. }
            | AfterCommand::MatterRow { .. },
            _,
        ) => {}
        (AfterCommand::Turn(tag), outcome) => turn::committed(actor, tag, outcome),
        (AfterCommand::Restore(matter), outcome) => {
            turn::restore_committed(actor, matter, outcome.is_ok());
        }
    }
    let result = outcome
        .map(|()| answer)
        .map_err(|failure| Error::Store(failure.error.clone()));
    actor.finish_command(id, result);
}

/// The thread `thread`, or `command.unknown_thread`.
pub(crate) fn check_thread(actor: &Actor, thread: ThreadId) -> Result<&ThreadEntry, Error> {
    actor.threads.get(&thread).ok_or_else(|| {
        Error::refused(
            ErrorCode::CommandUnknownThread,
            format!("no thread {thread}"),
        )
    })
}

/// The thread `thread` of a matter on the list, for a command that names it:
/// `command.unknown_thread` for an unknown thread or one of a forgotten matter.
pub(super) fn listed_thread(actor: &Actor, thread: ThreadId) -> Result<&ThreadEntry, Error> {
    let entry = actor.check_thread(thread)?;
    if actor.matters.contains_key(&entry.row.matter) {
        Ok(entry)
    } else {
        Err(Error::refused(
            ErrorCode::CommandUnknownThread,
            format!("thread {thread} is in a forgotten matter"),
        ))
    }
}

/// The matter `matter`, or `command.unknown_matter`.
pub(crate) fn check_matter(actor: &Actor, matter: MatterId) -> Result<&MatterEntry, Error> {
    actor.matters.get(&matter).ok_or_else(|| {
        Error::refused(
            ErrorCode::CommandUnknownMatter,
            format!("no matter {matter}"),
        )
    })
}

/// `CreateThread`: the agent must be in the settings; `ThreadCreated` and the row. No
/// launch.
fn create_thread(
    actor: &mut Actor,
    ctx: &CommandCtx,
    matter: MatterId,
    agent: clauseo_protocol::AgentId,
) -> Result<(), Error> {
    actor.check_matter(matter)?;
    if actor.effective.agent(agent.as_str()).is_none() {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            format!("no agent {agent} in the settings"),
        ));
    }
    let now = actor.now();
    let thread: ThreadId = actor.ids.mint(now.wall)?;
    let stream = StreamId::Thread(thread);
    let row = ThreadRow {
        id: thread,
        matter,
        agent: agent.clone(),
        // `CreateThread` takes no title, so the new row has neither a title nor a source.
        title: None,
        title_source: None,
        acp_session_id: None,
        archived: false,
        created: now.wall,
        updated: now.wall,
        seen_seq: None,
    };
    let event = DomainEvent::ThreadCreated(ThreadCreated {
        matter,
        agent,
        title: None,
    });
    let outcome = CommandOutcome::ThreadCreated { thread };
    let mut draft = Draft::new();
    actor.pipeline.push(
        &mut draft,
        stream,
        event,
        Some(ctx.id),
        now.wall,
        &mut actor.ids,
    )?;
    draft.thread_row(row.clone());
    draft.receipt(ctx.receipt(Some(stream), outcome, now.wall));
    let entry = ThreadEntry {
        row,
        header: None,
        last_stop: None,
        turn: turn::TurnState::default(),
    };
    let tag = Tag::Command {
        id: ctx.id,
        outcome,
        after: AfterCommand::Thread(Box::new(entry)),
    };
    actor.send_commit(draft, tag).map(|_| ())
}

/// `RenameThread` and `ArchiveThread`: the thread's stream is loaded first, so its header
/// stays current for `ListThreads`.
pub(super) fn thread_command(
    actor: &mut Actor,
    ctx: &CommandCtx,
    thread: ThreadId,
    change: ThreadChange,
) {
    if let Err(error) = listed_thread(actor, thread) {
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let ctx = *ctx;
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let done = loaded.and_then(|()| change_thread(actor, &ctx, thread, change));
            if let Err(error) = done {
                actor.finish_command(ctx.id, Err(error));
            }
            actor.release(stream);
        }),
    );
}

fn change_thread(
    actor: &mut Actor,
    ctx: &CommandCtx,
    thread: ThreadId,
    change: ThreadChange,
) -> Result<(), Error> {
    let before = actor.check_thread(thread)?.row.clone();
    if matches!(change, ThreadChange::Archive) && turn::turn_running(actor, thread) {
        return Err(Error::refused(
            ErrorCode::ThreadTurnRunning,
            "a turn is running; cancel it first",
        ));
    }
    let now = actor.now();
    let stream = StreamId::Thread(thread);
    let mut row = before.clone();
    row.updated = now.wall;
    let event = match change {
        ThreadChange::Rename(title, placeholder) => {
            let by = title_source::rename_source(placeholder);
            if title_source::title_may_change(before.title_source, before.title.is_some(), by) {
                row.title = Some(title.to_string());
                row.title_source = Some(by);
                let title = clauseo_protocol::CompactString::from(title.as_str());
                Some(DomainEvent::ThreadRenamed(ThreadRenamed { title, by }))
            } else {
                tracing::debug!(%thread, ?by, "a rename does not replace the thread's title");
                None
            }
        }
        ThreadChange::Archive if before.archived => None,
        ThreadChange::Archive => {
            row.archived = true;
            super::header_tools::forget(actor, thread);
            Some(DomainEvent::ThreadArchived(ThreadArchived {}))
        }
    };
    let outcome = CommandOutcome::Done;
    let mut draft = Draft::new();
    // Text the thread holds goes first, so it keeps its place before the rename.
    turn::flush_text(actor, thread, &mut draft)?;
    draft.receipt(ctx.receipt(Some(stream), outcome, now.wall));
    let after = match event {
        Some(event) => {
            actor.pipeline.push(
                &mut draft,
                stream,
                event,
                Some(ctx.id),
                now.wall,
                &mut actor.ids,
            )?;
            draft.thread_row(row.clone());
            if let Some(entry) = actor.threads.get_mut(&thread) {
                entry.row = row;
            }
            AfterCommand::Row {
                before: Box::new(before),
            }
        }
        // Already archived: nothing new, only the receipt.
        None => AfterCommand::Nothing,
    };
    let tag = Tag::Command {
        id: ctx.id,
        outcome,
        after,
    };
    actor.send_commit(draft, tag).map(|_| ())
}

/// What `RenameThread` or `ArchiveThread` changes.
#[derive(Debug)]
pub(super) enum ThreadChange {
    /// The title, and whether it is the app's first-words placeholder (else the lawyer typed it).
    Rename(Title, bool),
    Archive,
}

/// `command.conflict` for a `command_id` used with another command.
fn conflict(id: CommandId) -> Error {
    Error::refused(
        ErrorCode::CommandConflict,
        format!("command {id} was used for another command"),
    )
}
