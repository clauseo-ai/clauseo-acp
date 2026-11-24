// A message stopped before its turn started (its status sent, then removed: the host's TurnEnded { Cancelled } with no
// TurnStarted) gives its words back, never lost (Windows test, 24 Nov; #1302 review):
// - a later message of a conversation with an answer: into that conversation's field, or its draft when the field
//   isn't mounted (useUnsentRestore, in the composer);
// - a conversation the host stops listing because every message it had was stopped so (ThreadsChanged's removed, the
//   host's #1282): into a new draft of its matter (takeStopped, in conversations.ts), for every removed conversation,
//   open or not. The composer never opens that draft itself: an empty conversation alone is no removal.
// The line the composer shows says "because you stopped it" only after a Stop pressed in this window for that
// conversation (markStopped); otherwise it says plainly that the message wasn't sent.
import type { ViewStore } from "../../store/store.ts";
import { entryId, type StreamView } from "../../store/view.ts";
import type { EntryId, StreamId } from "../../transport/protocol.ts";
import { questionText } from "./live.ts";
import { useUserMessages } from "./steer-restore.ts";

const USER = "user_message:";

/** Why the words came back: a Stop pressed in this window, or no Stop of this window's (another reason, as far as
 * this window knows). */
export type Why = "stopped" | "neutral";

/** Messages seen sent, and messages given back: by entry id, for the app's life. */
const seenSent = new Set<EntryId>();
const given = new Set<EntryId>();
/** The conversations (their streams) whose Stop was pressed in this window, until their words come back, each with
 * the words of its messages not yet answered as the Stop was pressed: the removal may come once the lawyer has left
 * the conversation and the store no longer holds its view. */
const stoppedHere = new Map<StreamId, Map<EntryId, string>>();

/** The conversation keys whose field or draft holds words given back, and why: their composers say so until their
 * next send. For the app's life. */
export const unsentDrafts = new Map<string, Why>();

/** For the tests: forgets every message seen and given back, and every Stop. */
export function forgetUnsent(): void {
  seenSent.clear();
  given.clear();
  stoppedHere.clear();
  unsentDrafts.clear();
}

/** The lawyer pressed Stop (or its key) in this window for the conversation of `stream`, whose view is `v`. */
export function markStopped(stream: StreamId | null, v: StreamView | undefined): void {
  if (stream === null) return;
  const words = new Map<EntryId, string>();
  for (const id of v?.ids ?? []) {
    const m = v?.entries.get(id)?.user_message;
    if (v === undefined || m === undefined || m.delivery === "steer" || m.status !== "sent")
      continue;
    if (given.has(id) || v.entries.has(entryId("step", m.turn))) continue;
    words.set(id, questionText(id, m, v.texts));
  }
  stoppedHere.set(stream, words);
}

const whyOf = (stream: StreamId): Why => (stoppedHere.has(stream) ? "stopped" : "neutral");

/** Whether the view shows nothing but messages stopped before their turn (every message removed, no answer). */
const onlyRemoved = (v: StreamView): boolean =>
  v.ids.every((id) => id.startsWith(USER) && v.entries.get(id)?.user_message?.status === "removed");

/** A later message given back: its words and why. */
export type Unsent = { text: string; why: Why };

/** Each message of `stream`'s view that has just gone from sent to removed, in order, each once; and notes every
 * message still sent. When the view then shows nothing else, none: the host's removal of the conversation takes them
 * (takeStopped), so they aren't given back here into a conversation about to go. */
export function newlyUnsent(v: StreamView, stream: StreamId): Unsent[] {
  const ids: EntryId[] = [];
  for (const id of v.ids) {
    if (!id.startsWith(USER)) continue;
    const m = v.entries.get(id)?.user_message;
    if (m === undefined || m.delivery === "steer") continue;
    if (m.status === "sent") seenSent.add(id);
    else if (m.status === "removed" && seenSent.has(id) && !given.has(id)) ids.push(id);
  }
  if (ids.length === 0 || onlyRemoved(v)) return [];
  const why = whyOf(stream);
  stoppedHere.delete(stream);
  return ids.map((id) => {
    given.add(id);
    seenSent.delete(id);
    return { text: questionText(id, v.entries.get(id)!.user_message!, v.texts), why };
  });
}

/** The words of a conversation the host has stopped listing (ThreadsChanged's removed): each message not given back
 * yet, sent or removed, joined with blank lines, and why; null when there is none, or the view shows an answer (it
 * isn't that removal), or the store holds no view of it. Marks them given back. */
export function takeStopped(
  v: StreamView | undefined,
  stream: StreamId,
): { text: string; why: Why } | null {
  if (v === undefined) return takeKept(stream);
  if (v.ids.some((id) => !id.startsWith(USER))) return null;
  const words: string[] = [];
  for (const id of v.ids) {
    const m = v.entries.get(id)?.user_message;
    if (m === undefined || m.delivery === "steer" || given.has(id)) continue;
    if (m.status !== "sent" && m.status !== "removed") continue;
    given.add(id);
    seenSent.delete(id);
    words.push(questionText(id, m, v.texts));
  }
  if (words.length === 0) return null;
  const why = whyOf(stream);
  stoppedHere.delete(stream);
  return { text: words.join("\n\n"), why };
}

/** With no view of `stream`: the words kept as this window's Stop was pressed, not given back yet. */
function takeKept(stream: StreamId): { text: string; why: Why } | null {
  const kept = stoppedHere.get(stream);
  stoppedHere.delete(stream);
  const words = [...(kept ?? [])].filter(([id]) => !given.has(id));
  for (const [id] of words) given.add(id);
  return words.length === 0 ? null : { text: words.map(([, t]) => t).join("\n\n"), why: "stopped" };
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
