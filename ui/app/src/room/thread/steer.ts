// What a user message is in the transcript, and what an answer says of the tool calls a steer moved to the background
// (U7). Pure: the live rows and the older pages read the same rules.
//
// A message that waits (queued) or was taken back (removed) shows nowhere in the transcript: the strip holds it. A steer
// the agent took into the running answer (`injected`) is a mark inside that answer, where it arrived: only then, once
// the agent confirmed it. A steer not yet resolved is no mark, running turn or not: until the host says, nothing tells
// that the agent took it (it may yet go next, requeued, should the answer end or stop first), so it stands where the
// mark would, in the answer it was sent into, saying it sends when the agent finishes, with Send now (steer-rows.tsx).
// It moves once it resolves: into the mark, or to head its own exchange. A steer the host never delivered (`not_sent`:
// the answer stopped or ended, or the host restarted, first) stays in the same place, its words and that they were not
// sent, whatever its status (the host ends a held steer's queued turn, so it may read removed); its words also come
// back into the composer (steer-restore.ts).
// Every other sent message, a requeued or sent-now steer among them, is a question that heads its turn's exchange.
import type {
  EntryId,
  Timestamp,
  ToolCallEntry,
  TurnId,
  UserMessageEntry,
} from "../../transport/protocol.ts";

/** Where a user message shows: nowhere, as the mark of a steer taken in, as a steer still waiting on the agent's word
 * (`waiting`, in the answer it was sent into), as a steer never sent (`not_sent`, in the same place), or as a
 * question. */
export type AskedAs = "hidden" | "mark" | "waiting" | "not_sent" | "question";

/** Where a steer went: one of the four this version knows, "other" for an outcome from a newer Clauseo (a question,
 * then), or null while unresolved or for a message that was no steer. */
export type Outcome = "injected" | "requeued" | "sent_now" | "not_sent" | "other" | null;

export function outcomeOf(m: UserMessageEntry): Outcome {
  const s = m.steer;
  if (s === undefined) return null;
  if (typeof s === "string") return "other";
  if ("not_sent" in s) return "not_sent";
  if (s.injected !== undefined) return "injected";
  if (s.requeued !== undefined) return "requeued";
  return s.sent_now !== undefined ? "sent_now" : "other";
}

/** The running turn a steer joined, when the agent took it in. */
export const injectedTurn = (m: UserMessageEntry): TurnId | undefined =>
  typeof m.steer === "object" ? m.steer.injected?.turn : undefined;

/** How a user message shows: the grouping (exchanges.ts), the rows, live or on an older page, and the thread's break
 * read the same rule. */
export function askedAs(m: UserMessageEntry): AskedAs {
  const outcome = outcomeOf(m);
  if (outcome === "not_sent") return "not_sent";
  if (m.status === "queued" || m.status === "removed") return "hidden";
  if (outcome === "injected") return "mark";
  if (outcome === null && m.delivery === "steer") return "waiting";
  return "question";
}

/** What of a user message the grouping reads, as one string: a change elsewhere (its words) changes nothing. */
export const shapeOf = (m: UserMessageEntry | undefined): string =>
  m === undefined ? "" : `${m.status}|${m.turn}|${m.delivery ?? ""}|${outcomeOf(m) ?? ""}`;

/** The tool call's background mark, when a steer moved it there. */
export const backgroundOf = (call: ToolCallEntry) => call.background;

/** One note under an answer: the steered words' time, when the last call the steer moved to the background finished,
 * and each call with its finish time (the note's list names them: the note itself names none). */
export type SteerNoteData = {
  readonly steer: string;
  readonly at: Timestamp;
  readonly done: Timestamp;
  readonly found: readonly { readonly at: Timestamp; readonly what: string }[];
};

/** The notes of an answer's tool calls, one per steer, in the order of their first call. A steer shows its note only
 * once the host confirmed the agent took it in (outcome `injected`): the note says the answer uses what it found, which
 * is not true of words the agent never took. And only once its answer has ended (`endedAt`, its step's time) and every
 * call it moved finished at or before that time: until then each call's row says it carries on in the background, and
 * the note's "the answer uses what it found" would not be true (an answer can use only what finished before it
 * ended). `steerOf`: the steered message, or undefined when the window lacks it (no note then: its first words name
 * that message's time). */
export function steerNotes(
  calls: readonly ToolCallEntry[],
  steerOf: (steer: string) => UserMessageEntry | undefined,
  endedAt: Timestamp | undefined,
): SteerNoteData[] {
  if (endedAt === undefined) return [];
  const bySteer = new Map<string, ToolCallEntry[]>();
  for (const call of calls) {
    const mark = backgroundOf(call);
    if (mark === undefined) continue;
    const own = bySteer.get(mark.steer);
    if (own === undefined) bySteer.set(mark.steer, [call]);
    else own.push(call);
  }
  const out: SteerNoteData[] = [];
  for (const [steer, own] of bySteer) {
    const m = steerOf(steer);
    const at = m !== undefined && outcomeOf(m) === "injected" ? m.ts : undefined;
    const used = own.every((c) => {
      const done = backgroundOf(c)?.finished_at;
      return done !== undefined && done <= endedAt;
    });
    if (at === undefined || !used) continue;
    const found = own.map((c) => ({ at: backgroundOf(c)!.finished_at!, what: c.title }));
    out.push({
      steer,
      at,
      done: Math.max(...found.map((f) => f.at)),
      found,
    });
  }
  return out;
}

const USER = "user_message:";
/** The entry id of a steered message, from the message id a background mark names. */
export const steerEntry = (steer: string): EntryId => `${USER}${steer}`;
