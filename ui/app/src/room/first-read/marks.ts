// The first read's marks, one per matter, kept in this window's localStorage so a reload keeps them (the room keeps no
// other per-matter state of its own, and the host has no field for it yet: Backend gaps 3). A matter added during the
// first run is marked `added`; once its read starts, the mark names its thread and the turn of each first-read prompt
// `send_prompt` accepted (PromptAccepted's `turn`), Restart's too. The thread shows such a turn's question as
// "Clauseo's first read" and says how far the read has got by these turns, never by matching the prompt's text.
// Small and timer-free: the first run, the first read and the thread's chunk read it.
import type { MatterId, ThreadId, TurnId } from "../../transport/protocol.ts";

export const MARKS_KEY = "clauseo.first-read";

export type FirstReadMark = {
  /** Added during this computer's first run: the read may start. */
  added?: true;
  /** The read's thread, once made (or a "First read" the matter had already). */
  thread?: ThreadId;
  /** The first-read prompts' turns, in order. */
  turns?: TurnId[];
  /** The matter's files as the read started, for "Reading 24 files." */
  papers?: number;
};

type Marks = Readonly<Record<MatterId, FirstReadMark>>;

const storage = (): Pick<Storage, "getItem" | "setItem"> | null => {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
};

function load(): Marks {
  try {
    const text = storage()?.getItem(MARKS_KEY) ?? null;
    const read: unknown = text === null ? null : JSON.parse(text);
    return typeof read === "object" && read !== null ? (read as Marks) : {};
  } catch {
    return {};
  }
}

let marks: Marks | null = null;
const heard = new Set<() => void>();

/** Every mark, read once from storage. */
export const firstReadMarks = (): Marks => (marks ??= load());

/** Hears each change; returns the stop. */
export function subscribeMarks(listener: () => void): () => void {
  heard.add(listener);
  return () => void heard.delete(listener);
}

function put(matter: MatterId, patch: FirstReadMark): void {
  const all = firstReadMarks();
  marks = { ...all, [matter]: { ...all[matter], ...patch } };
  try {
    storage()?.setItem(MARKS_KEY, JSON.stringify(marks));
  } catch {
    // a full or refused storage: the mark holds for this window's life
  }
  for (const listener of heard) listener();
}

/** A matter added during the first run. */
export const markAdded = (matter: MatterId): void => {
  if (firstReadMarks()[matter]?.added !== true) put(matter, { added: true });
};

/** The read began: its thread, and later each accepted turn. */
export const markThread = (matter: MatterId, thread: ThreadId, papers?: number): void =>
  put(matter, papers === undefined ? { thread } : { thread, papers });

export const markTurn = (matter: MatterId, turn: TurnId): void =>
  put(matter, { turns: [...(firstReadMarks()[matter]?.turns ?? []), turn] });

/** The mark whose read runs in `thread`, with its matter; null for any other thread. */
export function markOfThread(
  thread: ThreadId | null,
  all: Marks = firstReadMarks(),
): { matter: MatterId; mark: FirstReadMark } | null {
  if (thread === null) return null;
  for (const [matter, mark] of Object.entries(all))
    if (mark.thread === thread) return { matter, mark };
  return null;
}

/** Whether a turn is a first-read prompt's. */
export function isFirstReadTurn(turn: TurnId): boolean {
  for (const mark of Object.values(firstReadMarks()))
    if (mark.turns?.includes(turn) === true) return true;
  return false;
}

/** For the review page and tests: these marks for this page alone, never written. */
export function seedMarks(seeded: Marks): void {
  marks = seeded;
  for (const listener of heard) listener();
}

/** For tests: forget the cached marks, so the next read loads storage again. */
export function forgetMarksForTests(): void {
  marks = null;
}
