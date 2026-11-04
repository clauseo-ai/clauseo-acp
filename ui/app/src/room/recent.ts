// Recent, pure (no React): every conversation in every matter (all-threads.ts), the scratch matter's too, archived
// ones left out as they are from their matter's list (U6), newest activity first, under its day in the lawyer's zone:
// "Today", "Yesterday", then "{weekday} {d} {month}" ("Tuesday 27 October", en-GB's order; hi-IN's in Hindi). The day
// and today are taken from the moment and `now` in `zone`, so nothing here reads the clock.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import type { MatterId, ThreadSummary, Timestamp } from "../transport/protocol.ts";
import type { MatterInfo } from "./host-next.ts";
import { dayOf, format } from "./when.ts";

export type RecentItem = { thread: ThreadSummary; matter: MatterInfo; at: Timestamp };
export type RecentGroup = { day: string; items: readonly RecentItem[] };

/** A v7 id's moment: its first 48 bits, the milliseconds it was made at; 0 for any other id. */
export function madeAt(id: string): Timestamp {
  const hex = id.replace(/-/g, "").slice(0, 12);
  return /^[0-9a-f]{12}$/i.test(hex) && id[14] === "7" ? Number.parseInt(hex, 16) : 0;
}

/** A day's heading, in the language: today, yesterday, or its weekday and date. */
export function dayHeading(at: Timestamp, now: Timestamp, zone: string, lang: Lang): string {
  const apart = dayOf(now, zone) - dayOf(at, zone);
  if (apart === 0) return frame(lang, "Today");
  if (apart === 1) return frame(lang, "Yesterday");
  const locale = lang === "hi" ? "hi-IN" : "en-GB";
  return format(locale, zone, { weekday: "long", day: "numeric", month: "long" })
    .format(at)
    .replace(",", "");
}

/** Every conversation not archived, newest activity first (then the newest made), under its day. */
export function recent(
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>,
  matters: readonly MatterInfo[],
  now: Timestamp,
  zone: string,
  lang: Lang,
): RecentGroup[] {
  const items: RecentItem[] = [];
  for (const matter of matters) {
    for (const thread of lists.get(matter.matter) ?? []) {
      if (thread.status === "archived") continue;
      items.push({ thread, matter, at: thread.last_activity ?? madeAt(thread.thread) });
    }
  }
  items.sort(
    (a, b) =>
      b.at - a.at ||
      (a.thread.thread < b.thread.thread ? 1 : a.thread.thread > b.thread.thread ? -1 : 0),
  );
  const groups: { day: string; items: RecentItem[] }[] = [];
  for (const item of items) {
    const day = dayHeading(item.at, now, zone, lang);
    const last = groups.at(-1);
    if (last !== undefined && last.day === day) last.items.push(item);
    else groups.push({ day, items: [item] });
  }
  return groups;
}
