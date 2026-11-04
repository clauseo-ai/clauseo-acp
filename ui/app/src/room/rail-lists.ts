// The rail's two lists beyond the date groups, as the parts take them: Recent's days (recent.ts) as RailRecent's rows,
// each pressed opening its conversation in its matter (open-conversation.ts), and the choice between Matters and
// Recent, kept for the session (no settings key holds it yet: `ui.rail_view` is asked of the host).
import type { RecentDay } from "@clauseo/components/clauseo/rail-recent.tsx";
import type { RailView } from "@clauseo/components/clauseo/rail-switch.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { t } from "@clauseo/components/lib/words.ts";
import type { MatterId, ThreadId, ThreadSummary, Timestamp } from "../transport/protocol.ts";
import { rowWords } from "./conversation-status.ts";
import type { MatterInfo } from "./host-next.ts";
import { recent } from "./recent.ts";
import { clockOf } from "./when.ts";

let chosen: RailView = "matters";
/** The rail's list, Matters or Recent, as the lawyer last chose it in this session. */
export const railView = {
  get: (): RailView => chosen,
  set: (view: RailView): void => {
    chosen = view;
  },
};

/** Recent's days as RailRecent takes them; `current` is the open conversation's thread. */
export function recentDays({
  lists,
  matters,
  current,
  now,
  zone,
  lang,
  onOpen,
}: {
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>;
  matters: readonly MatterInfo[];
  current: ThreadId | null;
  now: Timestamp;
  zone: string;
  lang: Lang;
  onOpen: (matter: MatterId, thread: ThreadId) => void;
}): RecentDay[] {
  const untitled = mt(lang, "New conversation");
  const titles = new Map<ThreadId, string>();
  for (const list of lists.values())
    for (const th of list) if (th.title !== undefined) titles.set(th.thread, th.title);
  return recent(lists, matters, now, zone, lang).map((g) => ({
    day: g.day,
    rows: g.items.map(({ thread, matter }) => {
      const words = rowWords(thread);
      const moment =
        words.state === "stopped" || words.state === "failed"
          ? clockOf(words.at, now, zone, lang)
          : null;
      // a stop on another day says nothing, as under a matter; a failure keeps its mark and says its day
      const state = words.state === "stopped" && moment?.day !== "" ? "idle" : words.state;
      return {
        id: thread.thread,
        title: thread.title ?? untitled,
        matter: matter.kind === "scratch" ? t(lang, "Scratch matter") : matter.name,
        state,
        ...(moment === null ? {} : { at: moment.time, day: moment.day }),
        ...(words.state === "waiting" ? { behind: titles.get(words.behind) ?? untitled } : {}),
        current: thread.thread === current,
        testId: "recent-row",
        onOpen: () => onOpen(matter.matter, thread.thread),
      };
    }),
  }));
}
