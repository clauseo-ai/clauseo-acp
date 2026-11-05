// What snooze and Done open later (U16b), in their own chunk (snooze-load.ts): the snooze menu's times in the lawyer's
// clock, the picker for a day and time of her own, a context menu's and a matter menu's two items, the line left in a
// row's place, the open Snoozed shelf's rows and the open Done fold's rows. The commands (snooze-acts.ts) load with it.
import { useState, type ReactNode } from "react";
import { DoneRows } from "@clauseo/components/clauseo/rail-done-rows.tsx";
import { ShelfRows, type ShelfRow } from "@clauseo/components/clauseo/rail-shelf-rows.tsx";
import { RailShelf } from "@clauseo/components/clauseo/rail-shelf.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { TriageItems } from "@clauseo/components/clauseo/rail-triage-items.tsx";
import { RowTriage } from "@clauseo/components/clauseo/rail-triage.tsx";
import { PointMenu } from "@clauseo/components/clauseo/rail-point-menu.tsx";
import { RailLine } from "@clauseo/components/clauseo/rail-line.tsx";
import { SnoozeItems, SnoozePicker } from "@clauseo/components/clauseo/snooze-menu.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import type { MatterId, ThreadSummary, Timestamp } from "../transport/protocol.ts";
import { whenWords, type ShelfOpens, type SnoozeCtx } from "./rail-snooze.tsx";
import { shelf, type Shelved } from "./shelf.ts";
import { snoozed, type Group } from "./triage.ts";
import type { MatterInfo } from "./host-next.ts";
import { snoozeChoices } from "./snooze-choices.ts";
import { MOST_AHEAD, pickedAt, pickerValue } from "./snooze-times.ts";

export type SnoozeMenuProps = {
  zone: string;
  /** The matter's next date, "2026-11-11", for Before the next date. */
  nextDate?: string | undefined;
  onPick: (until: Timestamp) => void;
  onChoose: () => void;
};

/** The menu's times as the lawyer's clock reads them when it opens (its popup mounts afresh each time). One whose time
 * has gone by while the menu stood open (9:00 pm, chosen at 9:01) sends nothing. */
export function SnoozeMenu({ zone, nextDate, onPick, onChoose }: SnoozeMenuProps) {
  const { lang } = useDisplay();
  const [now] = useState(Date.now);
  const pick = (until: Timestamp) => {
    if (takes(until)) onPick(until);
  };
  return (
    <SnoozeItems choices={snoozeChoices(now, zone, nextDate, lang, pick)} onChoose={onChoose} />
  );
}

export type PickerProps = {
  zone: string;
  onPick: (until: Timestamp) => void;
  onCancel: () => void;
};

/** A moment the host takes: after now, and no more than 366 days ahead. */
const takes = (until: Timestamp | null): until is Timestamp =>
  until !== null && until > Date.now() && until - Date.now() <= MOST_AHEAD;

/** The picker, from a minute ahead to the host's furthest; Snooze greyed for a value outside them. */
export function Picker({ zone, onPick, onCancel }: PickerProps) {
  const [now] = useState(Date.now);
  return (
    <SnoozePicker
      min={pickerValue(now + 60_000, zone)}
      max={pickerValue(now + MOST_AHEAD - 60_000, zone)}
      valid={(value) => takes(pickedAt(value, zone))}
      onPick={(value) => {
        const until = pickedAt(value, zone);
        if (takes(until)) onPick(until);
      }}
      onCancel={onCancel}
    />
  );
}

export type ShelfProps = {
  c: SnoozeCtx;
  matters: readonly MatterInfo[];
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>;
  open: ShelfOpens;
  group: ReadonlyMap<MatterId, Group>;
};

/** The Snoozed shelf: folded, how many and when the next is back; open, its rows. Nothing with nothing on it. */
export function Shelf({ c, matters, lists, open, group }: ShelfProps) {
  const shelved = shelf(matters, lists, c.now, group);
  const first = shelved[0];
  if (first === undefined) return null;
  return (
    <RailShelf
      count={shelved.length}
      next={whenWords(first.until, c.now, c.zone, c.lang)}
      rows={() => <ShelfOpen c={c} shelved={shelved} lists={lists} open={open} />}
      testId="rail-shelf"
    />
  );
}

type ShelfOpenProps = {
  c: SnoozeCtx;
  shelved: readonly Shelved[];
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>;
  open: ShelfOpens;
};

/** The open shelf: each conversation with its matter, or the matter snoozed whole, when it's back, and Wake now. */
function ShelfOpen({ c, shelved, lists, open }: ShelfOpenProps) {
  const when = (at: Timestamp) => whenWords(at, c.now, c.zone, c.lang);
  const rows = shelved.map(({ matter, thread, until }): ShelfRow => {
    const name = matter.kind === "scratch" ? t(c.lang, "Scratch matter") : matter.name;
    if (thread === null) {
      const asleep = (lists.get(matter.matter) ?? [])
        .filter((th) => th.status !== "archived" && snoozed(th, c.now))
        .map((th) => th.thread);
      return {
        key: matter.matter,
        title: name,
        matter: null,
        until: when(until),
        onOpen: () => open.matter(matter.matter),
        onWake: () => c.snooze.wake(asleep),
      };
    }
    return {
      key: thread.thread,
      title: thread.title ?? mt(c.lang, "New conversation"),
      matter: name,
      until: when(until),
      onOpen: () => open.conversation(matter.matter, thread.thread),
      onWake: () => c.snooze.wake([thread.thread]),
    };
  });
  return <ShelfRows rows={rows} />;
}

export { DoneRows, PointMenu, RailLine, RowTriage, TriageItems };
export type ItemsProps = { times: ReactNode; onDone: () => void; matter: boolean; done: boolean };
