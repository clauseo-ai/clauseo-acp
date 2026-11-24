// A message stopped before its turn started (its status sent, then removed: the host's TurnEnded { Cancelled } with no
// TurnStarted) gives its words back, never lost (Windows test, 24 Nov; #1302 review):
// - a later message of a conversation with an answer: into that conversation's field, or its draft when the field
//   isn't mounted (useUnsentRestore, in the composer);
// - a conversation the host stops listing because every message it had was stopped so (ThreadsChanged's removed, the
//   host's #1282): into a new draft of its matter (takeStopped, in conversations.ts), for every removed conversation,
//   open or not. The composer never opens that draft itself: an empty conversation alone is no removal.
// The line the composer shows says "because you stopped it" only after a Stop pressed in this window for that
// conversation while it had a message not answered (markStopped); otherwise it says plainly that the message wasn't
// sent.
import { useEffect, useRef } from "react";
import type { ViewStore } from "../../store/store.ts";
import { entryId, type StreamView } from "../../store/view.ts";
import type { EntryId, StreamId } from "../../transport/protocol.ts";
import { questionText } from "./live.ts";
import { useUserMessages } from "./steer-restore.ts";
import { causeOf, copiedIn, endedUnsent, givesBack, type Cause } from "./unsent-cause.ts";

const USER = "user_message:";

/** Why the words came back: a Stop pressed in this window, or no Stop of this window's (another reason, as far as
 * this window knows). */
export type Why = "stopped" | "neutral" | "sent_now" | "not_sent";

/** Messages seen sent and not answered (no step for their turn), each with its conversation's stream and its words:
 * kept until given back or answered, so a removal that comes once the store has dropped the view still has the words.
 * Filled only while a composer watches the conversation (or its Stop is pressed): a conversation never opened in this
 * window since the last reload isn't here. For the app's life. */
const seenSent = new Map<EntryId, { stream: StreamId; text: string }>();
/** Messages given back, by entry id. */
const given = new Set<EntryId>();
/** The conversations (their streams) whose Stop was pressed in this window while they had a message not answered;
 * cleared once its words come back, or once none is left unanswered (the turn ended: a Stop of a running answer). */
const stoppedHere = new Set<StreamId>();

/** The conversation keys whose field or draft holds words given back, and why: their composers say so until their
 * next send. For the app's life. */
export const unsentDrafts = new Map<string, Why>();

/** The mounted composers, by conversation key, that take words given back into their field. */
const takers = new Map<string, (text: string, why: Why) => void>();

/** Gives `text` to the mounted composer of the conversation `key`, after the words typed there: true when one took
 * them; false when none is mounted, when the caller keeps them in the draft store for the composer's mount. A draft
 * being typed in reads that store only as it mounts, and its next keystroke would save the field over it. */
export function giveToComposer(key: string, text: string, why: Why): boolean {
  const take = takers.get(key);
  if (take === undefined) return false;
  take(text, why);
  return true;
}

/** The composer of the conversation `key` takes words given back (giveToComposer) while mounted. */
export function useComposerTaker(key: string | null, take: (text: string, why: Why) => void): void {
  const latest = useRef(take);
  useEffect(() => {
    latest.current = take;
  });
  useEffect(() => {
    if (key === null) return;
    const taker = (text: string, why: Why) => latest.current(text, why);
    takers.set(key, taker);
    return () => {
      if (takers.get(key) === taker) takers.delete(key);
    };
  }, [key]);
}

/** For the tests: forgets every message seen and given back, and every Stop. */
export function forgetUnsent(): void {
  seenSent.clear();
  given.clear();
  stoppedHere.clear();
  unsentDrafts.clear();
}

/** Notes the view's messages sent and not answered, and forgets those answered since; whether any is left. */
function noteSent(v: StreamView, stream: StreamId): void {
  for (const id of v.ids) {
    if (!id.startsWith(USER)) continue;
    const m = v.entries.get(id)?.user_message;
    if (m === undefined || m.delivery === "steer" || given.has(id)) continue;
    if (v.entries.has(entryId("step", m.turn))) seenSent.delete(id);
    else if (m.status === "sent") seenSent.set(id, { stream, text: questionText(id, m, v.texts) });
  }
  if (![...seenSent.values()].some((s) => s.stream === stream)) stoppedHere.delete(stream);
}

/** The lawyer pressed Stop (or its key) in this window for the conversation of `stream`, whose view is `v`: noted
 * only while it has a message not answered, so a Stop of an answer that then ends says nothing of a later removal. */
export function markStopped(stream: StreamId | null, v: StreamView | undefined): void {
  if (stream === null || v === undefined) return;
  stoppedHere.add(stream);
  noteSent(v, stream);
}

const whyOf = (stream: StreamId): Why => (stoppedHere.has(stream) ? "stopped" : "neutral");

/** The wording for a message given back: the host's cause where it gives one (#1314): stopped as a Stop of this
 * window's, removed with no blame; else, with none, this window's Stop. */
function whyFor(cause: Cause | null, stream: StreamId): Why {
  if (cause === "stopped") return "stopped";
  if (cause === "removed") return "neutral";
  if (cause === "sent_now" || cause === "not_sent") return cause;
  return whyOf(stream);
}

/** Whether the view shows nothing but messages stopped before their turn (every message removed, no answer). */
const onlyRemoved = (v: StreamView): boolean =>
  v.ids.every((id) => {
    const m = id.startsWith(USER) ? v.entries.get(id)?.user_message : undefined;
    return m !== undefined && endedUnsent(m);
  });

/** A later message given back: its words and why. */
export type Unsent = { text: string; why: Why };

/** A message's cause, or "copied" when a queued copy names it (copy_of), whatever its own cause. */
type Ended = Cause | null | "copied";

/** Marks `ids` given back, with their words and the first cause's wording (`causes`, by id), and the conversation's
 * Stop as heard. One whose copy waits in the queue (a sign-in or an update, or a copy naming it) is marked and left
 * out: its words are the copy's. */
function give(
  stream: StreamId,
  ids: readonly EntryId[],
  causes: ReadonlyMap<EntryId, Ended> = new Map(),
): { text: string; why: Why } | null {
  const words: string[] = [];
  let why: Why | null = null;
  for (const id of ids) {
    const seen = seenSent.get(id);
    if (seen === undefined || given.has(id)) continue;
    given.add(id);
    seenSent.delete(id);
    const cause = causes.get(id) ?? null;
    if (cause === "copied" || !givesBack(cause)) continue;
    why ??= whyFor(cause, stream);
    words.push(seen.text);
  }
  stoppedHere.delete(stream);
  return words.length === 0 || why === null ? null : { text: words.join("\n\n"), why };
}

/** The causes of the view's messages `ids`; "copied" for one a queued copy names. */
function causesIn(v: StreamView, ids: readonly EntryId[]): Map<EntryId, Ended> {
  const copied = copiedIn(v);
  return new Map(
    ids.map((id) => {
      const m = v.entries.get(id)?.user_message;
      const ended: Ended = m === undefined ? null : copied.has(m.message) ? "copied" : causeOf(m);
      return [id, ended] as const;
    }),
  );
}

/** Each message of `stream`'s view that has just gone from sent to removed, in order, each once; and notes every
 * message still sent. When the view then shows nothing else, none: the host's removal of the conversation takes them
 * (takeStopped), so they aren't given back here into a conversation about to go. */
export function newlyUnsent(v: StreamView, stream: StreamId): Unsent[] {
  noteSent(v, stream);
  const ids = v.ids.filter((id) => {
    const m = v.entries.get(id)?.user_message;
    return m !== undefined && endedUnsent(m) && seenSent.has(id) && !given.has(id);
  });
  if (ids.length === 0 || onlyRemoved(v)) return [];
  const out: Unsent[] = [];
  for (const id of ids) {
    // each on its own: two messages ended for different causes say each its own words
    const got = give(stream, [id], causesIn(v, [id]));
    if (got !== null) out.push(got);
  }
  return out;
}

/** The words of a conversation the host has stopped listing (ThreadsChanged's removed): each message not given back
 * yet, sent or removed, joined with blank lines, and why; null when there is none, or the view shows an answer (it
 * isn't that removal). With no view in the store, the words seen while it was watched. Marks them given back. */
export function takeStopped(
  v: StreamView | undefined,
  stream: StreamId,
): { text: string; why: Why } | null {
  if (v === undefined) {
    const ids = [...seenSent].filter(([, s]) => s.stream === stream).map(([id]) => id);
    return give(stream, ids);
  }
  if (v.ids.some((id) => !id.startsWith(USER))) return null;
  const ids: EntryId[] = [];
  for (const id of v.ids) {
    const m = v.entries.get(id)?.user_message;
    if (m === undefined || m.delivery === "steer" || given.has(id)) continue;
    if (m.status !== "sent" && !endedUnsent(m)) continue;
    if (!seenSent.has(id)) seenSent.set(id, { stream, text: questionText(id, m, v.texts) });
    ids.push(id);
  }
  return give(stream, ids, causesIn(v, ids));
}

/** Watches the open thread's stream and hands each later message stopped before its turn started to `restore`. */
export function useUnsentRestore(
  store: ViewStore | null,
  stream: StreamId | null,
  restore: (unsent: Unsent) => void,
): void {
  useUserMessages(store, stream, (v) => {
    if (stream === null) return;
    for (const u of newlyUnsent(v, stream)) restore(u);
  });
}
