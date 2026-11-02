// What Versions holds for an open matter, as plain data the controller (controller.ts) makes and the parts read: where
// the room stands, what the dialog asks, what the room last did, and the rules for an undo's defaults. Pure.
import type {
  FileChange,
  FileChoice,
  RestorePreview,
  StepEntry,
  ThreadId,
  ThreadStatus,
  Timestamp,
} from "../../transport/protocol.ts";
import type { Timeline } from "./timeline.ts";

/** Where the room stands: at now, or looking back at a version, comparing it with the folder now or not. */
export type Look = { kind: "now" } | { kind: "back"; id: string; comparing: boolean };

/** An undo of one turn's change, with the lawyer's choice for each of its files. */
type UndoAsk = { act: "undo"; id: string; choices: ReadonlyMap<string, FileChoice> };
/** A restore of the matter to a version, with what restoring it writes now; undoes: the restore it takes back. */
type RestoreAsk = { act: "restore"; id: string; preview: RestorePreview; undoes: string | null };

/** What the restore dialog asks: to go ahead (failed: the code of a refusal that named no file, with Try again), or,
 * where a program holds a file, to close it and try again. */
export type Ask = (UndoAsk | RestoreAsk) &
  ({ stage: "confirm"; failed: string | null } | { stage: "word"; file: string });

/** What an act wrote: the version it made, or nothing (a bare done). */
export type Outcome = { kind: "wrote"; checkpoint: string } | { kind: "nothing" };

/** What the room last did, said under the band at now. */
export type Done =
  | { kind: "restored"; to: string; n: number; outcome: Outcome }
  | { kind: "undidRestore"; at: Timestamp }
  | { kind: "undidChange"; of: Timestamp; papers: readonly string[] };

/** The restore a restored line's Undo takes back: only one the host wrote and marked on the matter's stream. A restore
 * that found nothing to write answers with a version that was already there, and has no marker. */
export const restoreOf = (s: Snapshot, done: Done): string | null =>
  done.kind === "restored" &&
  done.outcome.kind === "wrote" &&
  s.restores.has(done.outcome.checkpoint)
    ? done.outcome.checkpoint
    : null;

/** Whether two previews would write the same: the same files, each changed the same way. */
export const sameChanges = (a: RestorePreview, b: RestorePreview): boolean =>
  a.changes.length === b.changes.length &&
  a.changes.every((c, i) => c.path === b.changes[i]?.path && c.change === b.changes[i]?.change);

/** A turn whose change couldn't be kept as a version: its conversation, its first file and when it ended. */
export type Unsaved = { thread: ThreadId; paper: string; ts: Timestamp };

export type Snapshot = {
  /** Whether the matter's first window has arrived. */
  ready: boolean;
  timeline: Timeline;
  look: Look;
  /** The conversation whose answer Restore and Undo wait for ("" untitled), or null while none runs. */
  busy: ThreadId | null;
  /** A refusal said an answer runs: Restore and Undo wait until list_threads, or a held conversation's header, says
   * none does. */
  refused: boolean;
  ask: Ask | null;
  done: Done | null;
  /** Whether the toolbar's Undo has a change to take back, or one still to page in. */
  canUndo: boolean;
  /** The code of the last act the host refused, for its words. */
  failed: string | null;
  unsaved: Unsaved | null;
  /** Comparing: the files that differ between the version and the folder now, null while they're asked for. */
  compare: { id: string; changes: readonly FileChange[] | null } | null;
  titles: ReadonlyMap<ThreadId, string>;
  /** Each restore the matter's stream has a marker for, to the version it restored. */
  restores: ReadonlyMap<string, string>;
  now: Timestamp;
  zone: string;
};

/** A thread working or waiting on the lawyer: Restore and Undo wait for it. */
export const isBusy = (s: ThreadStatus | undefined): boolean =>
  s === "starting" ||
  s === "running" ||
  s === "needs_you" ||
  (typeof s === "object" && "queued" in s);

/** The default choice for each file of a step's undo: a file that may hold the lawyer's own edits, or that changed
 * again since, is kept unless the lawyer says otherwise; every other file goes back. */
export function defaultChoices(
  step: StepEntry,
  since: ReadonlySet<string>,
): Map<string, FileChoice> {
  return new Map(
    step.files.map((f) => [
      f.path,
      f.may_include_lawyer_edits || since.has(f.path) ? "keep" : "undo",
    ]),
  );
}

/** The files of a step changed again by a later version, by path, with the newest version that did. */
export function changedSince(timeline: Timeline, id: string): Map<string, string> {
  const i = timeline.history.findIndex((v) => v.id === id);
  const out = new Map<string, string>();
  const v = timeline.history[i];
  if (v === undefined) return out;
  const mine = new Set([...v.touched, ...v.made]);
  for (const later of timeline.history.slice(i + 1)) {
    for (const p of [...later.touched, ...later.made, ...later.added])
      if (mine.has(p)) out.set(p, later.id);
  }
  return out;
}
