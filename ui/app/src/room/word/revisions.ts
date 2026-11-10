// A .docx's tracked changes, read and decided through the host: list_revisions reads them, decide_revisions accepts or
// rejects the ones named against the version the list was read from, and the latest decision is undone by restoring
// the file from the checkpoint the host took just before it (restore_version, this file only). Every send carries a
// command id of its own, so Try again is a new command. After each decision and undo the list is read again: the
// decided changes leave it. A file Word holds asks for the lock's dialog, whose Try again sends the same decision; every
// refused decision or undo reads the list again; a failed edit offers Try again. Not a component: the panel
// (changes-pane.tsx) reads it with useSyncExternalStore.
import { internalError, toHostError, type HostTransport } from "../../transport/host-transport.ts";
import type {
  CheckpointId,
  DocVersion,
  HostQuery,
  MatterId,
  QueryResult,
  RevisionDecision,
  RevisionList,
} from "../../transport/protocol.ts";

/** A decision as sent: the change ids, accept or reject, and the version of the list they were named from. */
export type Decision = { ids: readonly number[]; decision: RevisionDecision; version: DocVersion };

/**
 * The latest decision the host wrote: the checkpoint before it (Undo restores the file from it), when, and the version
 * of the list read right after it (null until that read).
 */
export type Decided = {
  before: CheckpointId;
  decision: RevisionDecision;
  at: number;
  version: DocVersion | null;
};

export type RevisionsView = {
  /** The list as last read; null before it is read and while the host can't read it. */
  list: RevisionList | null;
  /** When it was read: the "now" its changes' times are told against ("Today, 3:42 pm"). */
  readAt: number;
  /** list_revisions' refusal, by its code, while the list can't be read. */
  unread: string | null;
  /** A decision or an undo is on its way. */
  sending: boolean;
  /** What the last decision or undo was refused with, and the decision Try again sends again, where it offers one. */
  refused: { code: string; again: Decision | null } | null;
  /** The decision a file Word holds kept back: the lock's dialog is open while it is set. */
  locked: Decision | null;
  /** The dialog's act, kept past its closing, so its words don't change as it fades. */
  lockAct: RevisionDecision;
  /** The latest decision, while Undo can take it back here. */
  last: Decided | null;
};

const START: RevisionsView = {
  list: null,
  readAt: 0,
  unread: null,
  sending: false,
  refused: null,
  locked: null,
  lockAct: "accept",
  last: null,
};

/** The latest decision as a read of `version` leaves it: the read after it records the version, and a later read of
 * another version means the file changed after it, so Undo would undo more than it. */
function seen(last: Decided, version: DocVersion): Decided | null {
  if (last.version === null) return { ...last, version };
  return last.version === version ? last : null;
}

const LOCKED = "matter.file_locked";
const EDIT_FAILED = "word.edit_failed";

type ListRevisionsQuery = Extract<HostQuery, { list_revisions: unknown }>;

/** One file's tracked changes, in document order. */
export async function listRevisions(
  transport: HostTransport,
  matter: MatterId,
  path: string,
): Promise<RevisionList> {
  const query: ListRevisionsQuery = { list_revisions: { matter, path } };
  const answer: QueryResult = await transport.query(query);
  if (typeof answer !== "object" || !("revisions" in answer) || answer.revisions === undefined)
    throw internalError("list_revisions answered something else");
  return answer.revisions;
}

/** One panel's tracked changes: its file's list, its sends and what they came back with. */
export class Revisions {
  #view: RevisionsView = START;
  #listeners = new Set<() => void>();
  // a closed panel's answers change nothing; StrictMode's second mount opens it again
  #open = false;

  private readonly transport: HostTransport;
  private readonly matter: MatterId;
  private readonly path: string;

  constructor(transport: HostTransport, matter: MatterId, path: string) {
    this.transport = transport;
    this.matter = matter;
    this.path = path;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  view = (): RevisionsView => this.#view;

  /** Reads the list as the panel opens; the returned stop closes it. */
  start(): () => void {
    this.#open = true;
    void this.read();
    return () => {
      this.#open = false;
    };
  }

  #set(next: Partial<RevisionsView>): void {
    if (!this.#open) return;
    this.#view = { ...this.#view, ...next };
    for (const listener of this.#listeners) listener();
  }

  /** Reads the list again; a refusal leaves no list and its code. */
  async read(): Promise<void> {
    try {
      const list = await listRevisions(this.transport, this.matter, this.path);
      const { last } = this.#view;
      this.#set({
        list,
        readAt: Date.now(),
        unread: null,
        last: last === null ? null : seen(last, list.version),
      });
    } catch (error) {
      this.#set({ list: null, unread: toHostError(error).code });
    }
  }

  /** Accepts or rejects these changes of the list in view. */
  decide(ids: readonly number[], decision: RevisionDecision): void {
    const { list, sending } = this.#view;
    if (list === null || sending || ids.length === 0) return;
    void this.#send({ ids, decision, version: list.version });
  }

  /** The failed edit's Try again, or the lock dialog's: the same decision, as a new command. */
  again(): void {
    const decision = this.#view.locked ?? this.#view.refused?.again ?? null;
    if (decision !== null) void this.#send(decision);
  }

  /** The lock dialog's Cancel: nothing else changes. */
  closeLock(): void {
    this.#set({ locked: null });
  }

  async #send(d: Decision): Promise<void> {
    if (this.#view.sending) return;
    this.#set({ sending: true, refused: null });
    try {
      const outcome = await this.transport.dispatch({
        command_id: crypto.randomUUID(),
        command: {
          decide_revisions: {
            matter: this.matter,
            path: this.path,
            version: d.version,
            ids: [...d.ids],
            decision: d.decision,
          },
        },
      });
      if (typeof outcome !== "object" || outcome.revisions_decided === undefined)
        throw internalError("decide_revisions answered something else");
      const { before } = outcome.revisions_decided;
      this.#set({
        locked: null,
        last: { before, decision: d.decision, at: Date.now(), version: null },
      });
      await this.read();
    } catch (error) {
      const { code } = toHostError(error);
      if (code === LOCKED) this.#set({ locked: d, lockAct: d.decision });
      else this.#set({ locked: null, refused: { code, again: code === EDIT_FAILED ? d : null } });
      // whatever refused it, the panel shows the list as the file is now
      await this.read();
    } finally {
      this.#set({ sending: false });
    }
  }

  /** Takes the latest decision back: the file as it was just before it, then the list again. */
  async undo(): Promise<void> {
    const { last, sending } = this.#view;
    if (last === null || sending) return;
    this.#set({ sending: true, refused: null });
    try {
      await this.transport.dispatch({
        command_id: crypto.randomUUID(),
        command: {
          restore_version: {
            matter: this.matter,
            checkpoint: last.before,
            scope: { files: { paths: [this.path] } },
          },
        },
      });
      this.#set({ last: null });
      await this.read();
    } catch (error) {
      this.#set({ refused: { code: toHostError(error).code, again: null } });
      await this.read();
    } finally {
      this.#set({ sending: false });
    }
  }
}
