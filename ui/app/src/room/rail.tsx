// The room's rail: the parts' Rail with a MatterRow for every matter the host lists (matters.ts), the scratch matter
// only at its foot. With a NextHost, the date groups (when.ts), Pinned, Recent, finding by name and client, and each
// row's menu (Pin, Locate folder, Remove from the list); without one, one group, "No date yet", newest first, and
// finding by name only. What each row says it's doing comes from matters.ts's status, its moments put in the
// lawyer's clock as the rail renders, so nothing ticks at rest.
import type { Ref } from "react";
import { Rail, type MatterRowProps, type RailGroup } from "@clauseo/components/clauseo/rail.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { MatterInfo } from "./host-next.ts";
import type { Matters, MattersView, RowStatus } from "./matters.ts";
import { clockOf, dayOf, groupMatters } from "./when.ts";

/** A row's status as MatterRow takes it: the state, and its moment as words. Working never names a time (no end
 * estimates), so it says "Answering."; a stop on another day says its day before the time. */
function stateWords(
  status: RowStatus | undefined,
  now: number,
  zone: string,
  lang: Lang,
): Pick<MatterRowProps, "state" | "at" | "day"> {
  if (status === undefined) return { state: "idle" };
  if (status.at === undefined) return { state: status.state, at: "", day: "" };
  const { time, day } = clockOf(status.at, now, zone, lang);
  if (status.state === "stopped")
    return { state: "stopped", at: day === "" ? time : `${day} ${time}` };
  return { state: status.state, at: time, day };
}

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
}: {
  matters: Matters;
  view: MattersView;
  /** Matters, pressed below 1200 px: the rail stands open over the work. */
  open: boolean;
  onNewMatter: () => void;
  /** A matter was opened from the rail. */
  onMoved: () => void;
  newMatterRef: Ref<HTMLButtonElement>;
}) {
  const w = useWords();
  const { now, zone } = view;
  const detailed = view.detailed;
  const row = (m: MatterInfo): MatterRowProps => ({
    id: m.matter,
    name: m.name,
    note: "",
    hint: m.folder_display ?? "",
    current: view.open?.matter === m.matter,
    ...stateWords(view.status.get(m.matter), now, zone, w.lang),
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
  });
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
      find={{ all, recent }}
      scratch={
        scratch === undefined
          ? {}
          : { current: view.open?.matter === scratch.matter, onOpen: openScratch(scratch.matter) }
      }
      testIds={{ newMatter: "new-matter" }}
    />
  );
}
