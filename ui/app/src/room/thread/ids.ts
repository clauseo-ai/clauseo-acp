// Reading the thread's shape from its id list alone. Entry ids are "<kind>:<domain id>" (store/view.ts, entryId), so
// the turns and the latest question are found by prefix, without reading any entry: a flush that only appends text
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

const questions = new WeakMap<readonly EntryId[], EntryId | null>();

/** The latest question's id, or null. The view replaces its id list on every add or remove and never changes one in
 * place (store/view.ts), so it is kept per id list: each question row's pick on a flush reads it once. */
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
