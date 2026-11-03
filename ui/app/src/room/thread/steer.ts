// What a user message is in the transcript, and what an answer says of the tool calls a steer moved to the background
// (U7). Pure: the live rows and the older pages read the same rules.
//
// A message that waits (queued) or was taken back (removed) shows nowhere in the transcript: the strip holds it. A steer
// the agent took into the running answer (`injected`) is a mark inside that answer, where it arrived. A steer not yet
// resolved while a turn runs is shown the same way: claude-agent-acp takes most steers in, so the mark is where it will
// most likely stay, and should the host requeue it or send it now, it moves once, to head its own exchange. With no
// turn running there is nothing for it to join, so it reads as a question. Every other sent message, a requeued or
// sent-now steer among them, is a question that heads its turn's exchange.
import type {
  EntryId,
  Timestamp,
  ToolCallEntry,
  TurnId,
  UserMessageEntry,
} from "../../transport/protocol.ts";
import type { StreamView } from "../../store/view.ts";
import { turnRunning } from "./live.ts";

export type AskedAs = "hidden" | "mark" | "question";

/** Where a steer went: one of the three this version knows, "other" for an outcome from a newer Clauseo (a question,
 * then), or null while unresolved or for a message that was no steer. */
export type Outcome = "injected" | "requeued" | "sent_now" | "other" | null;

export function outcomeOf(m: UserMessageEntry): Outcome {
  const s = m.steer;
  if (s === undefined) return null;
  if (typeof s === "string") return "other";
  return s.injected !== undefined ? "injected" : s.requeued !== undefined ? "requeued" : "sent_now";
}

/** The running turn a steer joined, when the agent took it in. */
export const injectedTurn = (m: UserMessageEntry): TurnId | undefined =>
  typeof m.steer === "object" ? m.steer.injected?.turn : undefined;

/** Whether a turn runs in the view: the `running` every caller of askedAs passes, the grouping (exchanges.ts) and the
 * rows, live or on an older page, alike. A steer not yet resolved is a mark while a turn runs, wherever its entry came
 * from, else a question. */
export const runningIn = (v: StreamView | undefined): boolean =>
  turnRunning(v?.header?.header?.status);

export function askedAs(m: UserMessageEntry, running: boolean): AskedAs {
  if (m.status === "queued" || m.status === "removed") return "hidden";
  const outcome = outcomeOf(m);
  if (outcome === "injected") return "mark";
  if (outcome === null && m.delivery === "steer" && running) return "mark";
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
 * once its answer has ended (`endedAt`, its step's time) and every call it moved finished at or before that time:
 * until then each call's row says it carries on in the background, and the note's "the answer uses what it found"
 * would not be true (an answer can use only what finished before it ended). `askedAt`: the steered message's time,
 * or undefined when the window lacks it (no note then: its first words name that time). */
export function steerNotes(
  calls: readonly ToolCallEntry[],
  askedAt: (steer: string) => Timestamp | undefined,
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
    const at = askedAt(steer);
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
