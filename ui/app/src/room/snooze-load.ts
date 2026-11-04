// Snooze and Done's later chunk (snooze-late.tsx, with its words), loaded once (U16b): the pointer coming over the rail
// or the focus reaching it starts it, so a menu, the open shelf or the open fold finds it in or on its way. Read with
// React's use() on one cached promise, tagged once settled, as Find's chunk is (find-load.ts). A load that failed draws
// nothing and is tried again on the next ask.
import { createElement, use, type ComponentType } from "react";
import { late } from "@clauseo/components/lib/late.ts";
// types alone: erased, so this file has no static dependency on the chunk
import type { ItemsProps, PickerProps, ShelfOpenProps, SnoozeMenuProps } from "./snooze-late.tsx";
import type { Acts, Line } from "./snooze-acts.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { DoneRow } from "@clauseo/components/clauseo/rail-done-rows.tsx";

type Parts = {
  SnoozeMenu: ComponentType<SnoozeMenuProps>;
  Picker: ComponentType<PickerProps>;
  ShelfOpen: ComponentType<ShelfOpenProps>;
  DoneRows: ComponentType<{ rows: readonly DoneRow[] }>;
  Items: ComponentType<ItemsProps>;
  Line: ComponentType<{ text: string; onUndo: () => void }>;
  acts: (transport: HostTransport, show: (key: string, line: Line) => void) => Acts;
};

type Chunk = Promise<Parts> & { status?: "fulfilled"; value?: Parts };

const NOTHING = () => null;
const NOOP = () => {};
const NONE: Parts = {
  SnoozeMenu: NOTHING,
  Picker: NOTHING,
  ShelfOpen: NOTHING,
  DoneRows: NOTHING,
  Items: NOTHING,
  Line: NOTHING,
  acts: () => ({ snooze: NOOP, done: NOOP, undo: NOOP, wake: NOOP, notDone: NOOP }),
};

let chunk: Chunk | null = null;

/** Starts loading the chunk, once; resolves when it is in memory. */
export function preloadSnooze(): Promise<Parts> {
  if (chunk !== null) return chunk;
  const loading: Chunk = late(
    Promise.all([import("./snooze-late.tsx"), import("./snooze-acts.ts")]),
  ).then(
    ([m, a]): Parts => ({
      SnoozeMenu: m.SnoozeMenu,
      Picker: m.Picker,
      ShelfOpen: m.ShelfOpen,
      DoneRows: m.DoneRows,
      Items: m.TriageItems,
      Line: m.RailLine,
      acts: a.snoozeActs,
    }),
    (error: unknown): Parts => {
      console.warn(`Snooze's chunk did not load: ${String(error)}`);
      if (chunk === loading) chunk = null;
      return NONE;
    },
  );
  void late(
    loading.then((value) => {
      loading.status = "fulfilled";
      loading.value = value;
    }),
  );
  chunk = loading;
  return loading;
}

export const LazySnoozeMenu = (p: SnoozeMenuProps) =>
  createElement(use(preloadSnooze()).SnoozeMenu, p);
export const LazyPicker = (p: PickerProps) => createElement(use(preloadSnooze()).Picker, p);
export const LazyShelfOpen = (p: ShelfOpenProps) =>
  createElement(use(preloadSnooze()).ShelfOpen, p);
export const LazyDoneRows = (p: { rows: readonly DoneRow[] }) =>
  createElement(use(preloadSnooze()).DoneRows, p);
export const LazyItems = (p: ItemsProps) => createElement(use(preloadSnooze()).Items, p);
export const LazyLine = (p: { text: string; onUndo: () => void }) =>
  createElement(use(preloadSnooze()).Line, p);
