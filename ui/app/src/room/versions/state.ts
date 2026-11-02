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

/** What the restore dialog asks. */
export type Ask =
  | { act: "undo"; stage: "confirm"; id: string; choices: ReadonlyMap<string, FileChoice> }
  | { act: "restore"; stage: "confirm"; id: string; preview: RestorePreview }
  | {
      act: "undo" | "restore";
      stage: "word";
      id: string;
      file: string;
      /** For an undo: the lawyer's choices, kept for Try again. */
      choices?: ReadonlyMap<string, FileChoice>;
    };

/** What the room last did, said under the band at now. */
export type Done =
  | { kind: "restored"; to: string; n: number; checkpoint: string }
  | { kind: "undidRestore"; at: Timestamp }
  | { kind: "undidChange"; of: Timestamp; papers: readonly string[] };

/** A turn whose change couldn't be kept as a version: its conversation, its first file and when it ended. */
export type Unsaved = { thread: ThreadId; paper: string; ts: Timestamp };

export type Snapshot = {
  /** Whether the matter's first window has arrived. */
  ready: boolean;
  timeline: Timeline;
  look: Look;
  /** The conversation whose answer Restore and Undo wait for ("" untitled), or null while none runs. */
  busy: ThreadId | null;
  /** A refusal said an answer runs: Restore and Undo wait until the matter's stream moves on. */
  refused: boolean;
  ask: Ask | null;
  done: Done | null;
  /** The code of the last act the host refused, for its words. */
  failed: string | null;
  unsaved: Unsaved | null;
  /** Comparing: the files that differ between the version and the folder now, null while they're asked for. */
  compare: { id: string; changes: readonly FileChange[] | null } | null;
  titles: ReadonlyMap<ThreadId, string>;
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
