// Reading the thread's shape from its id list alone. Entry ids are "<kind>:<domain id>" (store/view.ts, entryId), so
// the turns and the latest output are found by prefix, without reading any entry: a flush that only appends text
// leaves the id list, and so everything derived here, the same.
import type { EntryId } from "../../transport/protocol.ts";

const QUESTION = "user_message:";
const ANSWER_TEXT = "agent_text:";

export const isQuestion = (id: EntryId): boolean => id.startsWith(QUESTION);
export const isAgentText = (id: EntryId): boolean => id.startsWith(ANSWER_TEXT);

/** The ids by exchange: each starts at a question; a leading run before any question (a window that opens mid-turn)
 * is its own exchange, without one. */
export function exchangesOf(ids: readonly EntryId[]): EntryId[][] {
  const out: EntryId[][] = [];
  for (const id of ids) {
    const last = out.at(-1);
    if (last === undefined || isQuestion(id)) out.push([id]);
    else last.push(id);
  }
  return out;
}

/** What the agent's latest output can't be: the lawyer's own words, and what the host writes around a turn. */
const NOT_OUTPUT = [QUESTION, "notice:", "marker:", "version:"];

/** Each id list's latest output. The view replaces its id list on every add or remove and never changes one in place
 * (store/view.ts), so a list's answer holds for its life: every answer row's pick on a flush reads it once. */
const outputs = new WeakMap<readonly EntryId[], EntryId | null>();

/** The agent's latest output: the last id that is none of NOT_OUTPUT. The stand-in for #123's `latestOutput` on the
 * view (open PR #123); switch to it when #123 merges, since this file can't change store/view.ts. */
export function latestOutput(ids: readonly EntryId[]): EntryId | null {
  const known = outputs.get(ids);
  if (known !== undefined) return known;
  const found = scanOutput(ids);
  outputs.set(ids, found);
  return found;
}

function scanOutput(ids: readonly EntryId[]): EntryId | null {
  for (let i = ids.length - 1; i >= 0; i -= 1) {
    const id = ids[i]!;
    if (!NOT_OUTPUT.some((prefix) => id.startsWith(prefix))) return id;
  }
  return null;
}

const questions = new WeakMap<readonly EntryId[], EntryId | null>();

/** The latest question's id, or null; kept per id list, as `latestOutput` is. */
export function latestQuestion(ids: readonly EntryId[]): EntryId | null {
  const known = questions.get(ids);
  if (known !== undefined) return known;
  let found: EntryId | null = null;
  for (let i = ids.length - 1; i >= 0 && found === null; i -= 1)
    if (isQuestion(ids[i]!)) found = ids[i]!;
  questions.set(ids, found);
  return found;
}

/** Whether two id lists hold the same ids in the same order. */
export function sameIds(a: readonly EntryId[], b: readonly EntryId[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) if (a[i] !== b[i]) return false;
  return true;
}
