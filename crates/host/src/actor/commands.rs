//! Commands: shape, principal, receipt, state, in that order; one commit per command carrying
//! its receipt; concurrent duplicates sharing the first answer.
//!
//! A command waits in [`Actor::commands`] from its receipt lookup until its answer, so a
//! duplicate that arrives meanwhile joins it (same hash) or is refused (`command.conflict`).
//! A refused command stores no receipt. A failed commit answers with the stream's first
//! failure.

use clauseo_protocol::{
    CommandEnvelope, CommandId, CommandOutcome, DomainEvent, Envelope, ErrorCode, HostCommand,
    HostError, MatterId, Principal, RenameSource, StreamId, ThreadArchived, ThreadCreated,
    ThreadId, ThreadRenamed, Timestamp, ValidatedCommand,
};
use clauseo_store::{Receipt, ThreadRow, command_hash};

use super::input::Reply;
use super::matters::PendingMatter;
use super::{Actor, MatterEntry, Tag, ThreadEntry, turn};
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
    /// A turn command's continuation.
    Turn(turn::TurnTag),
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
            if !matches!(actor.phase, super::Phase::Serving) {
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
    let Some(pending) = actor.commands.remove(&id) else {
        return;
    };
    let mut waiters = pending.waiters.into_iter();
    let Some(first) = waiters.next() else {
        return;
    };
    let shared = result
        .as_ref()
        .map(|outcome| *outcome)
        .map_err(|error| HostError::new(error.code(), error.to_string()));
    let _gone = first.send(result);
    for waiter in waiters {
        let _gone = waiter.send(shared.clone().map_err(Error::Refused));
    }
}

/// The state checks and the work of one command.
fn run_command(actor: &mut Actor, ctx: &CommandCtx, command: HostCommand) {
    match command {
        HostCommand::CreateMatter { name, source } => {
            super::matters::create_matter(actor, ctx, &name, source);
        }
        HostCommand::CreateThread { matter, agent } => {
            let done = create_thread(actor, ctx, matter, agent);
            if let Err(error) = done {
                actor.finish_command(ctx.id, Err(error));
            }
        }
        HostCommand::RenameThread { thread, title } => {
            let title = clauseo_protocol::CompactString::from(title.as_str());
            thread_command(actor, ctx, thread, ThreadChange::Rename(title));
        }
        HostCommand::ArchiveThread { thread } => {
            thread_command(actor, ctx, thread, ThreadChange::Archive);
        }
        HostCommand::SendPrompt { thread, .. }
        | HostCommand::Cancel { thread }
        | HostCommand::AnswerPermission { thread, .. }
        | HostCommand::AnswerElicitation { thread, .. }
        | HostCommand::SetConfigOption { thread, .. }
        | HostCommand::SetMode { thread, .. }
        | HostCommand::UndoStep { thread, .. } => match actor.check_thread(thread) {
            Ok(_) => turn::command(actor, ctx, thread, command),
            Err(error) => actor.finish_command(ctx.id, Err(error)),
        },
        HostCommand::EditQueuedPrompt { .. }
        | HostCommand::RemoveQueuedPrompt { .. }
        | HostCommand::SendQueuedNow { .. }
        | HostCommand::ResumeQueue { .. }
        | HostCommand::RestartAgent { .. }
        | HostCommand::RestoreVersion { .. } => {
            let refusal = Error::refused(ErrorCode::CommandInvalid, "not built yet");
            actor.finish_command(ctx.id, Err(refusal));
        }
    }
}

/// A command's commit returned.
pub(super) fn command_committed(
    actor: &mut Actor,
    id: CommandId,
    answer: CommandOutcome,
    after: AfterCommand,
    outcome: Result<&[Envelope], &Failure>,
) {
    match (after, outcome) {
        (AfterCommand::Matter(entry), Ok(_)) => {
            actor.matters.insert(entry.row.id, *entry);
        }
        (AfterCommand::Thread(entry), Ok(_)) => {
            actor.threads.insert(entry.row.id, *entry);
        }
        (AfterCommand::Row { before }, Err(_)) => {
            if let Some(entry) = actor.threads.get_mut(&before.id) {
                entry.row = *before;
            }
        }
        (
            AfterCommand::Nothing
            | AfterCommand::Matter(_)
            | AfterCommand::Thread(_)
            | AfterCommand::Row { .. },
            _,
        ) => {}
        (AfterCommand::Turn(tag), outcome) => turn::committed(actor, tag, outcome),
    }
    let result = outcome
        .map(|_| answer)
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
        title: None,
        acp_session_id: None,
        archived: false,
        created: now.wall,
        updated: now.wall,
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
fn thread_command(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId, change: ThreadChange) {
    if let Err(error) = actor.check_thread(thread) {
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
        ThreadChange::Rename(title) => {
            row.title = Some(title.to_string());
            Some(DomainEvent::ThreadRenamed(ThreadRenamed {
                title,
                by: RenameSource::Lawyer,
            }))
        }
        ThreadChange::Archive if before.archived => None,
        ThreadChange::Archive => {
            row.archived = true;
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
enum ThreadChange {
    Rename(clauseo_protocol::CompactString),
    Archive,
}

/// `command.conflict` for a `command_id` used with another command.
fn conflict(id: CommandId) -> Error {
    Error::refused(
        ErrorCode::CommandConflict,
        format!("command {id} was used for another command"),
    )
}
