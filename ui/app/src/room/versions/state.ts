// What Versions holds for an open matter, as plain data the controller (controller.ts) makes and the parts read: where
// the room stands, what the dialog asks, what the room last did, and the rules for an undo's defaults. Pure.
import type {
  CommandOutcome,
  FileChange,
  FileChoice,
  RestorePreview,
  SkippedFile,
  StepEntry,
  ThreadId,
  ThreadStatus,
  Timestamp,
} from "../../transport/protocol.ts";
import type { OneFile } from "../matter-stream.ts";
import type { Timeline } from "./timeline.ts";

/** Where the room stands: at now (id: the newest version, a turn's, picked on the timeline for its Undo), or looking
 * back at a version, comparing it with the folder now or not. */
export type Look = { kind: "now"; id?: string } | { kind: "back"; id: string; comparing: boolean };

/** An undo of one turn's change, with the lawyer's choice for each of its files; edited: the files the host says changed
 * since the turn's version (preview_undo, as the thread's Undo asks it), once it answers. */
type UndoAsk = {
  act: "undo";
  id: string;
  choices: ReadonlyMap<string, FileChoice>;
  edited?: ReadonlySet<string>;
  /** The host hasn't answered yet: Undo waits. */
  checking?: boolean;
};
/** A restore of the matter to a version, with what restoring it writes now; undoes: the restore it takes back; file:
 * the one file Versions' per-file Restore puts back, null for the whole matter. */
type RestoreAsk = {
  act: "restore";
  id: string;
  preview: RestorePreview;
  undoes: string | null;
  file: string | null;
  /** The matter moved while the dialog was open and what restoring writes changed (the list was asked again). */
  moved?: boolean;
};

/** What the restore dialog asks: to go ahead (failed: the code of a refusal that named no file, with Try again), or,
 * where a program holds a file, to close it and try again. */
export type Ask = (UndoAsk | RestoreAsk) &
  ({ stage: "confirm"; failed: string | null } | { stage: "word"; files: readonly string[] });

/** What an act wrote: the version it made, or nothing (a bare done). */
export type Outcome = { kind: "wrote"; checkpoint: string } | { kind: "nothing" };

/** What the host's answer to an act says it wrote. */
export function outcomeOf(r: CommandOutcome): Outcome {
  if (typeof r === "object" && "step_undone" in r)
    return { kind: "wrote", checkpoint: r.step_undone.checkpoint };
  if (typeof r === "object" && "version_restored" in r)
    return { kind: "wrote", checkpoint: r.version_restored.checkpoint };
  return { kind: "nothing" };
}

/** Paths in the order a version's row names its papers: made, changed, then added; any it doesn't name go last. */
export function inRowOrder(t: Timeline, id: string, paths: readonly string[]): string[] {
  const v = t.history.find((x) => x.id === id);
  const order = v === undefined ? [] : [...v.made, ...v.touched, ...v.added];
  const rank = (p: string) => {
    const i = order.indexOf(p);
    return i < 0 ? order.length : i;
  };
  return [...paths].sort((a, b) => rank(a) - rank(b));
}

/** What the room last did, said under the band at now. */
export type Done =
  | { kind: "restored"; to: string; n: number; outcome: Outcome }
  /** One file put back as it was at a version, by Versions' per-file Restore; moved: it changed while the dialog was
   * open, and the copy it had then is kept in Versions. */
  | { kind: "restoredFile"; to: string; file: string; outcome: Outcome; moved?: boolean }
  /** file: the one file the restore undone had put back (Versions' Restore this document), null for the matter. */
  | {
      kind: "undidRestore";
      at: Timestamp;
      file: string | null;
      /** The version the undo answered with, whose marker says what it did to that file. */
      checkpoint?: string;
    }
  | {
      kind: "undidChange";
      of: Timestamp;
      papers: readonly string[];
      /**  The files the lawyer chose to undo that stay as they are: Clauseo had no earlier copy of
       * them. */
      kept?: readonly SkippedFile[];
    };

/** Whether restoring writes nothing: no file changed or removed (the file is the same, or the host keeps it). */
export const writesNothing = (p: RestorePreview): boolean =>
  p.changes.length === 0 && (p.removes ?? []).length === 0;

/** What a restore that went through did: undid the restore it undoes (that file alone, for a one-file restore's undo,
 * its marker saying what it did to it), restored the matter, or restored one file (moved: it changed while the dialog
 * was open; its marker says whether the restore wrote it). */
export function restoreDone(
  ask: Pick<RestoreAsk, "id" | "preview" | "undoes" | "file" | "moved">,
  outcome: Outcome,
  at: Timestamp,
): Done {
  const { file, preview } = ask;
  if (ask.undoes !== null) {
    if (file === null) return { kind: "undidRestore", at, file };
    const checkpoint = outcome.kind === "wrote" ? { checkpoint: outcome.checkpoint } : {};
    return { kind: "undidRestore", at, file, ...checkpoint };
  }
  if (file === null) return { kind: "restored", to: ask.id, n: preview.changes.length, outcome };
  const moved = ask.moved === true && outcome.kind === "wrote";
  return { kind: "restoredFile", to: ask.id, file, outcome, ...(moved ? { moved } : {}) };
}

/** The restore a restored line's Undo takes back: only one the host wrote and marked on the matter's stream. A restore
 * that found nothing to write answers with a version that was already there, and has no marker. */
export const restoreOf = (s: Snapshot, done: Done): string | null =>
  (done.kind === "restored" || done.kind === "restoredFile") &&
  done.outcome.kind === "wrote" &&
  s.restores.has(done.outcome.checkpoint)
    ? done.outcome.checkpoint
    : null;

/** The file a compared document's Restore this document puts back: a changed or removed one by its path, one moved or
 * renamed since by its old path (it comes back there, and the new one stays). One added since wasn't there to bring
 * back, and a change from a newer Clauseo isn't known: null. */
export function restorable(f: FileChange): string | null {
  if (f.change === "modified" || f.change === "deleted") return f.path;
  return typeof f.change === "object" && "renamed" in f.change ? f.change.renamed.from : null;
}

/** The restore dialog with what restoring writes, asked again: at confirm, moved once the list differs from one shown. */
export function askedAgain(
  ask: Extract<Ask, { act: "restore" }>,
  preview: RestorePreview,
): Extract<Ask, { act: "restore"; stage: "confirm" }> {
  const { act, id, undoes, file } = ask;
  const moved = ask.moved === true || !sameChanges(ask.preview, preview);
  return { act, id, preview, undoes, file, moved, stage: "confirm", failed: null };
}

/** Whether two previews would write the same: the same files, each changed the same way over the same content
 * there now (a paper saved again is another change, though still "modified"), and the same files removed. */
export const sameChanges = (a: RestorePreview, b: RestorePreview): boolean =>
  a.changes.length === b.changes.length &&
  a.changes.every((c, i) => {
    const o = b.changes[i];
    return c.path === o?.path && c.change === o.change && c.before === o.before;
  }) &&
  (a.removes ?? []).join("\n") === (b.removes ?? []).join("\n");

/** A turn whose change couldn't be kept as a version: its conversation, its first file and when it ended. */
export type Unsaved = { thread: ThreadId; paper: string; ts: Timestamp };

/** What a file/folder clash names for its words: the entry in the way, by its matter path (its own name is said), and
 * whether it's a folder that now has the paper's name (else a file where its folder was). */
export type Clash = { at: string; folder: boolean };

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
  /** What the last matter.file_folder_clash refusal named, for its words; null when it named nothing. */
  clash: Clash | null;
  unsaved: Unsaved | null;
  /** Comparing: the files that differ between the version and the folder now, null while they're asked for. */
  compare: { id: string; changes: readonly FileChange[] | null } | null;
  titles: ReadonlyMap<ThreadId, string>;
  /** Each restore the matter's stream has a marker for, to the version it restored. */
  restores: ReadonlyMap<string, string>;
  /** Each restore of one file the matter's stream has a marker for, to that file (and whether it deleted it for
   * good). */
  oneFile: ReadonlyMap<string, OneFile>;
  now: Timestamp;
  zone: string;
};

/** Versions as it opens: at now, before the matter's first window. */
export const freshSnapshot = (now: Timestamp, zone: string): Snapshot => ({
  ready: false,
  timeline: { history: [], meta: new Map() },
  look: { kind: "now" },
  busy: null,
  refused: false,
  ask: null,
  done: null,
  canUndo: true,
  failed: null,
  clash: null,
  unsaved: null,
  compare: null,
  titles: new Map(),
  restores: new Map(),
  oneFile: new Map(),
  now,
  zone,
});

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

/** A newest version picked at now (for its Undo) once a newer one lands: looked back at, as any older version is; one
 * the timeline no longer holds leaves now bare. */
export function staleNewest(look: Look, timeline: Timeline): Look {
  if (look.kind !== "now" || look.id === undefined || look.id === timeline.history.at(-1)?.id)
    return look;
  return timeline.meta.has(look.id)
    ? { kind: "back", id: look.id, comparing: false }
    : { kind: "now" };
}

/** The undo dialog after the matter moved: each file a newer version changed is kept, unless the lawyer chose for it
 * (`chosen`); null when nothing changes. The room knows the files of the versions it can read only: a later turn of a
 * conversation it doesn't hold, or a later restore, lists none until every version carries its files (H12). */
export function keepChangedSince(
  timeline: Timeline,
  id: string,
  choices: ReadonlyMap<string, FileChoice>,
  chosen: ReadonlySet<string>,
): Map<string, FileChoice> | null {
  let next: Map<string, FileChoice> | null = null;
  for (const path of changedSince(timeline, id).keys()) {
    if (chosen.has(path) || choices.get(path) !== "undo") continue;
    next ??= new Map(choices);
    next.set(path, "keep");
  }
  return next;
}

/** The undo dialog for version `id` as it opens: Undo waits while the host checks what changed since. */
export const undoAsk = (id: string, choices: ReadonlyMap<string, FileChoice>): Ask => ({
  act: "undo",
  id,
  choices,
  stage: "confirm",
  failed: null,
  checking: true,
});

/** The undo dialog for version `id` once the host answered which files changed since that version (edited; null: it
 * refused or there was no conversation to ask): each file named is kept, unless the lawyer chose for it (`chosen`), as
 * a file a newer version changed is. A file the turn added and the lawyer edited after it would otherwise go with the
 * lawyer's edits. Undo waits for the answer (checking); with none, each file's flag decides, as in the thread. Null when
 * the dialog is no longer that undo's. */
export function keepEdited(
  ask: Ask | null,
  id: string,
  edited: readonly string[] | null,
  chosen: ReadonlySet<string>,
): Ask | null {
  if (ask?.act !== "undo" || ask.id !== id) return null;
  if (edited === null || ask.stage !== "confirm") return { ...ask, checking: false };
  const choices = new Map(ask.choices);
  for (const path of edited)
    if (!chosen.has(path) && choices.get(path) === "undo") choices.set(path, "keep");
  return { ...ask, choices, edited: new Set(edited), checking: false };
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
