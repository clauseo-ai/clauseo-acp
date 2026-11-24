// A steer the host never delivered (outcome `not_sent`: the answer stopped or ended, or the host restarted, before
// the agent took the words in) comes back into the composer as a draft, so the lawyer's words are never lost (U7). Only
// as it happens: a steer this app saw with any other outcome (waiting, requeued, queued at the head) and then sees as
// not sent. One already not sent when its entry first comes (a conversation opened again, a reset's window, an older
// page, which this never reads) stays where it is, in the transcript. Each steer comes back once, by its entry id, for
// the app's life: a composer mounted again, or a reset that replays the entry, restores nothing more.
// A message stopped before its turn started (its status sent, then removed: the host's TurnEnded { Cancelled } with no
// TurnStarted, as when Stop is pressed while it waits for the matter) comes back the same way, with whether it was the
// conversation's only message, when the composer opens a new draft with it instead (Windows test, 24 Nov).
import { useEffect, useRef } from "react";
import type { ViewStore } from "../../store/store.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, StreamId, UserMessageEntry } from "../../transport/protocol.ts";
import { questionText } from "./live.ts";
import { outcomeOf } from "./steer.ts";

const USER = "user_message:";

/** Steers seen before they were not sent, and steers already given back: by entry id, for the app's life. */
const seenOpen = new Set<EntryId>();
const restored = new Set<EntryId>();

/** Messages seen sent, and messages stopped before their turn and already given back: by entry id. */
const seenSent = new Set<EntryId>();
const unsentBack = new Set<EntryId>();

/** For the tests: forgets every steer and message seen and given back. */
export function forgetSteerRestores(): void {
  seenOpen.clear();
  restored.clear();
  seenSent.clear();
  unsentBack.clear();
  unsentDrafts.clear();
}

/** The drafts that hold a message stopped before its turn started, its own conversation having nothing else: their
 * composers say it wasn't sent until their next send. By conversation key, for the app's life. */
export const unsentDrafts = new Set<string>();

/** A message stopped before its turn started: its words, and whether the conversation shows nothing else (every
 * message it has was stopped so, and no answer), when there is no conversation left to put it back in. */
export type Unsent = { text: string; only: boolean };

/** The words of a conversation the host has just stopped listing because every message it had was stopped before its
 * turn started (ThreadsChanged's removed): each message not given back yet, sent or removed, the view showing no
 * answer, joined with blank lines; null when there is none, or the view shows an answer (it isn't that removal). Marks
 * them given back, so the composer, if its patch comes after, gives back nothing more. */
export function takeStopped(v: StreamView | undefined): string | null {
  if (v === undefined || v.ids.some((id) => !id.startsWith(USER))) return null;
  const words: string[] = [];
  for (const id of v.ids) {
    const m = v.entries.get(id)?.user_message;
    if (m === undefined || m.delivery === "steer" || unsentBack.has(id)) continue;
    if (m.status !== "sent" && m.status !== "removed") continue;
    unsentBack.add(id);
    seenSent.delete(id);
    words.push(questionText(id, m, v.texts));
  }
  return words.length === 0 ? null : words.join("\n\n");
}

/** Each message of the view that has just gone from sent to removed, in order, each once; and notes every message
 * still sent. Pure but for the two sets. */
export function newlyUnsent(v: StreamView): Unsent[] {
  const out: Unsent[] = [];
  for (const id of v.ids) {
    if (!id.startsWith(USER)) continue;
    const m = v.entries.get(id)?.user_message;
    if (m === undefined || m.delivery === "steer") continue;
    if (m.status === "sent") seenSent.add(id);
    if (m.status !== "removed" || !seenSent.has(id) || unsentBack.has(id)) continue;
    unsentBack.add(id);
    seenSent.delete(id);
    out.push({ text: questionText(id, m, v.texts), only: false });
  }
  if (out.length === 0) return out;
  const only = v.ids.every(
    (id) => id.startsWith(USER) && v.entries.get(id)?.user_message?.status === "removed",
  );
  return out.map((u) => ({ ...u, only }));
}

const isSteer = (m: UserMessageEntry | undefined): m is UserMessageEntry =>
  m !== undefined && m.delivery === "steer";

/** The words of each steer of the view that has just turned not sent, in order, each once; and notes every steer
 * still open. Pure but for the two sets. */
export function newlyNotSent(v: StreamView): string[] {
  const out: string[] = [];
  for (const id of v.ids) {
    if (!id.startsWith(USER)) continue;
    const m = v.entries.get(id)?.user_message;
    if (!isSteer(m)) continue;
    if (outcomeOf(m) !== "not_sent") {
      seenOpen.add(id);
      continue;
    }
    if (!seenOpen.has(id) || restored.has(id)) continue;
    restored.add(id);
    seenOpen.delete(id);
    out.push(questionText(id, m, v.texts));
  }
  return out;
}

/** Watches the open thread's stream and hands each steer's words to `restore` as it turns not sent. */
export function useNotSentRestore(
  store: ViewStore | null,
  stream: StreamId | null,
  restore: (text: string) => void,
): void {
  useUserMessages(store, stream, (v) => {
    for (const text of newlyNotSent(v)) restore(text);
  });
}

/** Watches the open thread's stream and hands each message stopped before its turn started to `restore`. */
export function useUnsentRestore(
  store: ViewStore | null,
  stream: StreamId | null,
  restore: (unsent: Unsent) => void,
): void {
  useUserMessages(store, stream, (v) => {
    for (const u of newlyUnsent(v)) restore(u);
  });
}

/** Calls `changed` with the view of `stream` as its user messages change. Reads the view only when its ids or a user
 * message's entry changed, so a streamed chunk costs one identity check per message. */
function useUserMessages(
  store: ViewStore | null,
  stream: StreamId | null,
  changed: (v: StreamView) => void,
): void {
  const give = useRef(changed);
  useEffect(() => {
    give.current = changed;
  });
  useEffect(() => {
    if (store === null || stream === null) return;
    let ids: readonly EntryId[] = [];
    let own: readonly EntryId[] = [];
    let users: (UserMessageEntry | undefined)[] = [];
    const check = () => {
      const v = store.view(stream);
      if (v === undefined) return;
      if (v.ids !== ids) {
        ids = v.ids;
        own = ids.filter((id) => id.startsWith(USER));
      }
      const now = own.map((id) => v.entries.get(id)?.user_message);
      if (now.length === users.length && now.every((m, i) => m === users[i])) return;
      users = now;
      give.current(v);
    };
    check();
    return store.subscribe(check);
  }, [store, stream]);
}
