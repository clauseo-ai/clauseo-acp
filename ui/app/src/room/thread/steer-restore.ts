// A steer the host never delivered (outcome `not_sent`: the answer stopped or ended, or the host restarted, before
// the agent took the words in) comes back into the composer as a draft, so the lawyer's words are never lost (U7). Only
// as it happens: a steer this app saw with any other outcome (waiting, requeued, queued at the head) and then sees as
// not sent. One already not sent when its entry first comes (a conversation opened again, a reset's window, an older
// page, which this never reads) stays where it is, in the transcript. Each steer comes back once, by its entry id, for
// the app's life: a composer mounted again, or a reset that replays the entry, restores nothing more.
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

/** For the tests: forgets every steer seen and given back. */
export function forgetSteerRestores(): void {
  seenOpen.clear();
  restored.clear();
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

/** Watches the open thread's stream and hands each steer's words to `restore` as it turns not sent. Reads the view
 * only when its ids or a user message's entry changed, so a streamed chunk costs one identity check per message. */
export function useNotSentRestore(
  store: ViewStore | null,
  stream: StreamId | null,
  restore: (text: string) => void,
): void {
  const give = useRef(restore);
  useEffect(() => {
    give.current = restore;
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
      for (const text of newlyNotSent(v)) give.current(text);
    };
    check();
    return store.subscribe(check);
  }, [store, stream]);
}
