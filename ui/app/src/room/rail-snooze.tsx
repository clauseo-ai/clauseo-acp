// Snooze and Done on the rail (U16b), the room's half: the commands (H-triage), the line left in a row's place after
// either until the pointer or the focus leaves the rail, the picker, and the props each part takes: a conversation's
// buttons and its snoozed mark, the open matter's done conversations for its fold, a matter's menu, and the Snoozed
// shelf. A line shows only once the host has answered Done for every thread; one refused (a time past, or more than
// 366 days ahead) puts back those it took and leaves the rows as they were. What opens later (the menu's times, the
// picker, the open shelf and fold) is snooze-late.tsx's, loaded by snooze-load.ts.
import { Suspense, useEffect, useState, type ReactNode } from "react";
import type { ConversationRowProps } from "@clauseo/components/clauseo/rail-conversations.tsx";
import type { DoneRow } from "@clauseo/components/clauseo/rail-done-rows.tsx";
import type { MatterRowProps } from "@clauseo/components/clauseo/rail.tsx";
import type { Triage } from "@clauseo/components/clauseo/rail-triage.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tw } from "@clauseo/components/lib/words-triage.ts";
import { clock } from "@clauseo/components/lib/words.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId, ThreadId, ThreadSummary, Timestamp } from "../transport/protocol.ts";
import type { ConversationRow } from "./conversations.ts";
import type { MatterInfo } from "./host-next.ts";
import {
  LazyDoneRows,
  LazyButtons,
  LazyItems,
  LazyLine,
  LazyPicker,
  LazyPoint,
  LazyShelf,
  LazySnoozeMenu,
  preloadSnooze,
} from "./snooze-load.ts";
import type { Acts, Held, Line } from "./snooze-acts.ts";
import { snoozed, wholly, type Group } from "./triage.ts";
import { clockOf, dayOf } from "./when.ts";

const NO_LINES: ReadonlyMap<string, Line> = new Map();

/** A time to come back, as the rail says it: "6:00 pm", "tomorrow 9:00 am", "Monday 9:00 am", "18 November 9:00 am". */
export function whenWords(at: Timestamp, now: Timestamp, zone: string, lang: Lang): string {
  const { time, day } = clockOf(at, now, zone, lang);
  // the time holds together on its line: "9:00 am" never breaks before "am"
  const held = clock(lang, time).replace(/ (am|pm)$/, "\u00a0$1");
  if (day === "") return held;
  return dayOf(at, zone) - dayOf(now, zone) === 1
    ? tw(lang, "tomorrow {time}", { time: held })
    : `${day} ${held}`;
}

/** The rail's snooze and Done: the lines standing in rows' places (by thread, or by matter), the row whose picker is
 * open, and the acts. */
export function useSnooze(transport: HostTransport, onRail: () => boolean) {
  const [lines, setLines] = useState(NO_LINES);
  const [picking, setPicking] = useState<{ key: string; threads: readonly Held[] } | null>(null);
  // a line comes only while the pointer or the focus is on the rail: one that arrives after both have left would never
  // be cleared, and would hold the rows where they stand
  const show = (key: string, line: Line) => {
    if (onRail()) setLines((was) => new Map(was).set(key, line));
  };
  // the window coming back clears the lines, as the pointer or the focus leaving the rail does
  useEffect(() => {
    const clear = () => {
      if (document.visibilityState === "visible") setLines(NO_LINES);
    };
    const focused = (e: FocusEvent) => {
      if (!(e.target instanceof Node)) setLines(NO_LINES);
    };
    window.addEventListener("focus", focused);
    document.addEventListener("visibilitychange", clear);
    return () => {
      window.removeEventListener("focus", focused);
      document.removeEventListener("visibilitychange", clear);
    };
  }, []);
  // the commands come with the later chunk, in or on its way since the pointer came over the rail
  const acts = (run: (acts: Acts) => void) =>
    void preloadSnooze().then((parts) => run(parts.acts(transport, show)));
  return {
    lines,
    picking,
    snooze: (key: string, threads: readonly Held[], until: Timestamp) => {
      setPicking(null);
      acts((a) => a.snooze(key, threads, until));
    },
    done: (key: string, threads: readonly Held[]) => acts((a) => a.done(key, threads)),
    undo: (key: string) => {
      const line = lines.get(key);
      if (line === undefined) return;
      const left = new Map(lines);
      left.delete(key);
      setLines(left);
      acts((a) => a.undo(line));
    },
    wake: (threads: readonly ThreadId[]) => acts((a) => a.wake(threads)),
    notDone: (thread: ThreadId) => acts((a) => a.notDone(thread)),
    choose: (key: string, threads: readonly Held[]) => setPicking({ key, threads }),
    cancel: () => setPicking(null),
    /** The pointer or the focus left the rail: the lines go, their rows taking their new places. */
    leave: () => {
      if (lines.size > 0) setLines(NO_LINES);
    },
  };
}

export type Snooze = ReturnType<typeof useSnooze>;
/** What the props are made with: the acts, and the clock the rail renders with. */
export type SnoozeCtx = { snooze: Snooze; now: Timestamp; zone: string; lang: Lang };

/** Snooze and Done for these threads: the snooze menu's times, the menus' two items, and Mark (all) done. */
function triageOf(
  c: SnoozeCtx,
  key: string,
  threads: readonly Held[],
  nextDate: string | undefined,
  done = false,
): Triage {
  const first = threads[0];
  const onDone =
    done && first !== undefined
      ? () => c.snooze.notDone(first.thread)
      : () => c.snooze.done(key, threads);
  const menu = () => (
    <Suspense fallback={null}>
      <LazySnoozeMenu
        zone={c.zone}
        nextDate={nextDate}
        onPick={(until) => c.snooze.snooze(key, threads, until)}
        onChoose={() => c.snooze.choose(key, threads)}
      />
    </Suspense>
  );
  const triage: Triage = {
    menu,
    onDone,
    buttons: (title) => (
      <Suspense fallback={null}>
        <LazyButtons triage={triage} title={title} />
      </Suspense>
    ),
    items: (matter) => (
      <Suspense fallback={null}>
        <LazyItems times={menu()} onDone={onDone} matter={matter} done={done} />
      </Suspense>
    ),
    context: (at, onClose) => (
      <Suspense fallback={null}>
        <LazyPoint at={at} onClose={onClose}>
          {triage.items(false)}
        </LazyPoint>
      </Suspense>
    ),
  };
  return triage;
}

/** What stands in a row's place, if anything: its picker, or its line with Undo. */
function replaceOf(c: SnoozeCtx, key: string): ReactNode | undefined {
  const s = c.snooze;
  if (s.picking?.key === key) {
    const threads = s.picking.threads;
    return (
      <Suspense fallback={null}>
        <LazyPicker
          zone={c.zone}
          onPick={(until) => s.snooze(key, threads, until)}
          onCancel={s.cancel}
        />
      </Suspense>
    );
  }
  const line = s.lines.get(key);
  if (line === undefined) return undefined;
  const text =
    line.kind === "done"
      ? tw(c.lang, "Marked as done")
      : tw(c.lang, "Snoozed until {when}", {
          when: whenWords(line.until, c.now, c.zone, c.lang),
        });
  return (
    <Suspense fallback={null}>
      <LazyLine text={text} onUndo={() => s.undo(key)} />
    </Suspense>
  );
}

/** The open matter's rows (`props`, made from `rows`, one each): those that stay in its list, a snoozed one ending in
 * the alarm clock, each with Snooze and Mark as done, and its done ones for the fold. A row's line keeps it in place
 * until the pointer leaves; the open one stays in the list, done or not. */
export function splitConversations(
  c: SnoozeCtx,
  rows: readonly ConversationRow[],
  props: readonly ConversationRowProps[],
  nextDate: string | undefined,
): { rows: ConversationRowProps[]; done?: { count: number; rows: () => ReactNode } } {
  const shown: ConversationRowProps[] = [];
  const done: DoneRow[] = [];
  rows.forEach((row, i) => {
    const p = props[i];
    const thread = row.thread;
    if (p === undefined) return;
    if (thread === null) {
      shown.push(p);
      return;
    }
    const replace = replaceOf(c, thread);
    if (row.done === true && replace === undefined && p.current !== true) {
      done.push({
        id: p.id,
        title: p.title,
        ...(p.onOpen === undefined ? {} : { onOpen: p.onOpen }),
        onNotDone: () => c.snooze.notDone(thread),
      });
      return;
    }
    const until = snoozed(row, c.now) ? row.snoozed_until : undefined;
    const held: Held =
      row.snoozed_until === undefined ? { thread } : { thread, snoozed_until: row.snoozed_until };
    // the open one, done, says so, and its menu offers Mark as not done
    const isDone = row.done === true;
    shown.push({
      ...p,
      ...(until === undefined
        ? {}
        : {
            state: "idle",
            snoozed: tw(c.lang, "Snoozed until {when}", {
              when: whenWords(until, c.now, c.zone, c.lang),
            }),
          }),
      triage: triageOf(c, thread, [held], nextDate, isDone),
      ...(isDone ? { done: true } : {}),
      ...(replace === undefined ? {} : { replace }),
    });
  });
  if (done.length === 0) return { rows: shown };
  const fold = () => (
    <Suspense fallback={null}>
      <LazyDoneRows rows={done} />
    </Suspense>
  );
  return { rows: shown, done: { count: done.length, rows: fold } };
}

/** A matter's Snooze until… and Mark all done, for its conversations not done (none: no items), and its line. */
export function matterSnooze(
  c: SnoozeCtx,
  m: MatterInfo,
  list: readonly ThreadSummary[] | undefined,
  group: Group | undefined,
): Pick<MatterRowProps, "triage" | "replace" | "snoozed"> {
  const awake = (list ?? []).filter((th) => th.status !== "archived" && th.done !== true);
  const replace = replaceOf(c, m.matter);
  // snoozed whole, yet in view (Pinned, or held where it stood): its row says so
  const whole = group === "snoozed" || m.kind === "scratch" ? null : wholly(list, c.now);
  return {
    ...(whole === null
      ? {}
      : {
          snoozed: tw(c.lang, "Snoozed until {when}", {
            when: whenWords(whole, c.now, c.zone, c.lang),
          }),
        }),
    ...(awake.length === 0 || m.kind === "scratch"
      ? {}
      : {
          triage: triageOf(c, m.matter, awake, m.next_date),
        }),
    ...(replace === undefined ? {} : { replace }),
  };
}

/** Where a shelf row's press goes: its conversation, or the matter snoozed whole. */
export type ShelfOpens = {
  conversation: (m: MatterId, th: ThreadId) => void;
  matter: (m: MatterId) => void;
};

/** The Snoozed shelf (snooze-late.tsx's, loaded with the later chunk), or nothing with no conversation snoozed. */
export function snoozedShelf(
  c: SnoozeCtx,
  matters: readonly MatterInfo[],
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>,
  open: ShelfOpens,
  group: ReadonlyMap<MatterId, Group>,
): ReactNode {
  if (
    ![...lists.values()].some((l) => l.some((th) => th.status !== "archived" && snoozed(th, c.now)))
  )
    return undefined;
  return (
    <Suspense fallback={null}>
      <LazyShelf c={c} matters={matters} lists={lists} open={open} group={group} />
    </Suspense>
  );
}
