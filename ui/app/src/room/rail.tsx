// The room's rail: the parts' Rail with a MatterRow for every matter the host lists (matters.ts), the scratch matter
// only at its foot. With the NextHost's matters part, the date groups (when.ts), Pinned, Recent (the five opened last),
// finding by name and client, and each row's menu (Pin, Locate folder, Remove from the list); without it, one group,
// "No date yet", newest first, and finding by name only. What each row says it's doing comes from matters.ts's status,
// its moments put in the lawyer's clock as the rail renders, so nothing ticks at rest; the open matter's row says
// nothing, since the lawyer is looking at it. Until the list arrives, and if it can't be read, the rail draws no list
// and no scratch row.
import type { Ref } from "react";
import { Rail, type MatterRowProps, type RailGroup } from "@clauseo/components/clauseo/rail.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { isolate } from "@clauseo/components/lib/bidi.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { MatterInfo } from "./host-next.ts";
import type { Matters, MattersView, RowStatus } from "./matters.ts";
import { clockOf, dayOf, groupMatters } from "./when.ts";

/** A row's status as MatterRow takes it: the state, and its moment as words. Working never names a time (no end
 * estimates), so it says "Answering."; a stop or a reset on another day gives its day. */
function stateWords(
  status: RowStatus | undefined,
  now: number,
  zone: string,
  lang: Lang,
): Pick<MatterRowProps, "state" | "at" | "day"> {
  if (status === undefined) return { state: "idle" };
  if (status.at === undefined) return { state: status.state, at: "", day: "" };
  const { time, day } = clockOf(status.at, now, zone, lang);
  return { state: status.state, at: time, day };
}

/** How many matters Recent shows: the five opened last. */
const RECENT = 5;

/** The newest first, by id: v7 ids sort by creation. */
const newestFirst = (a: MatterInfo, b: MatterInfo) =>
  a.matter < b.matter ? 1 : a.matter > b.matter ? -1 : 0;

export function RoomRail({
  matters,
  view,
  open,
  onNewMatter,
  onMoved,
  newMatterRef,
  onEscape,
}: {
  matters: Matters;
  view: MattersView;
  /** Matters, pressed below 1200 px: the rail stands open over the work. */
  open: boolean;
  onNewMatter: () => void;
  /** A matter was opened from the rail. */
  onMoved: () => void;
  newMatterRef: Ref<HTMLButtonElement>;
  /** Escape inside the rail. */
  onEscape: () => void;
}) {
  const w = useWords();
  const { now, zone } = view;
  const detailed = view.detailed;
  const row = (m: MatterInfo): MatterRowProps => {
    const current = view.open?.matter === m.matter;
    return {
      id: m.matter,
      name: m.name,
      note: "",
      hint:
        m.folder_display === undefined || m.folder_display === "" ? "" : isolate(m.folder_display),
      current,
      // the menu's Locate folder follows the folder, apart from the status: the open row says nothing, yet offers it
      moved: m.folder_state === "missing",
      // the matter the lawyer is looking at says nothing: its work is in view
      ...(current
        ? { state: "idle" as const }
        : stateWords(view.status.get(m.matter), now, zone, w.lang)),
      testIds: { row: "matter-row" },
      onOpen: () => {
        matters.open(m.matter);
        onMoved();
      },
      ...(detailed
        ? {
            pinned: m.pinned,
            onPin: (pinned: boolean) => void matters.pin(m.matter, pinned),
            onRemove: () => void matters.forget(m.matter),
            ...(m.folder_state === "missing"
              ? { onLocate: () => void matters.locate(m.matter) }
              : {}),
          }
        : {}),
    };
  };
  const folders = view.matters.filter((m) => m.kind !== "scratch");
  const scratch = view.matters.find((m) => m.kind === "scratch");
  const pinned = detailed ? folders.filter((m) => m.pinned) : [];
  const groups: RailGroup[] = detailed
    ? groupMatters(
        folders.filter((m) => !m.pinned),
        dayOf(now, zone),
      ).map((g) => ({ when: frame(w.lang, g.when), matters: g.matters.map(row) }))
    : [
        {
          when: frame(w.lang, "No date yet"),
          matters: [...folders].sort(newestFirst).map(row),
        },
      ];
  const recent = detailed
    ? folders
        .filter((m) => m.last_opened !== undefined)
        .sort((a, b) => (b.last_opened ?? 0) - (a.last_opened ?? 0))
        .slice(0, RECENT)
        .map(row)
    : [];
  const all = folders.map((m) => ({
    ...row(m),
    client: detailed ? (m.client ?? "") : "",
    caseNo: "",
  }));
  const openScratch = (id: MatterId) => () => {
    matters.open(id);
    onMoved();
  };
  return (
    <Rail
      groups={groups}
      pinned={pinned.map(row)}
      open={open}
      onNewMatter={onNewMatter}
      newMatterRef={newMatterRef}
      // nothing to find on an empty rail
      {...(folders.length === 0
        ? {}
        : {
            find: { all, recent, covers: detailed ? ("nameClient" as const) : ("name" as const) },
          })}
      {...(scratch === undefined
        ? {}
        : {
            scratch: {
              current: view.open?.matter === scratch.matter,
              onOpen: openScratch(scratch.matter),
            },
          })}
      empty={view.phase === "ready"}
      onEscape={onEscape}
      testIds={{ newMatter: "new-matter", find: "find" }}
    />
  );
}
