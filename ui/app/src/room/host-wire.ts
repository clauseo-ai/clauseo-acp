// H7 over the desktop host: the room's SettingsHost (host-next.ts) from the transport's `query`, `dispatch` and app feed
// (`onAppEvent`). The room keeps its own spellings; this file is the one place they meet the wire's: lite's "full" and
// "lite" are the wire's "off" and "on", motion's "reduced" is "reduce", and the text size is a number on the wire.
import { DEFAULT_DISPLAY, TEXT_SIZES, type TextSize } from "@clauseo/components/lib/display.ts";
import {
  internalError,
  type AppFeed,
  type AppFeedSource,
  type HostTransport,
} from "../transport/host-transport.ts";
import type {
  Lite,
  Motion,
  SettingsView,
  UiPatch,
  UiSettings as WireUi,
} from "../transport/protocol.ts";
import type { Settings, SettingsHost, UiLite, UiMotion, UiSettings } from "./host-next.ts";

/** What the wire settings host reaches. */
export type WireSettingsSource = Pick<HostTransport, "query" | "dispatch"> & AppFeedSource;

/** One table per field, room to wire; the way back is derived from it. */
const LITE: Record<UiLite, Lite> = { automatic: "automatic", full: "off", lite: "on" };
const MOTION: Record<UiMotion, Motion> = { system: "system", full: "full", reduced: "reduce" };

const inverse = <R extends string, W extends string>(table: Record<R, W>): Record<W, R> =>
  Object.fromEntries(Object.entries(table).map(([room, wire]) => [wire, room])) as Record<W, R>;
const LITE_BACK = inverse(LITE);
const MOTION_BACK = inverse(MOTION);

/** The wire's text size as the room's; one outside the room's sizes (the host holds 14 to 20) is the default. */
const sizeOf = (points: number): TextSize =>
  TEXT_SIZES.find((size) => size === points) ?? DEFAULT_DISPLAY.textSize;

/** The host's settings as the room spells them; the locked keys as they came. */
export function roomSettingsOf(view: SettingsView): Settings {
  const ui: WireUi = view.ui;
  return {
    ui: {
      theme: ui.theme,
      font_size: sizeOf(ui.font_size),
      language: ui.language,
      lite: LITE_BACK[ui.lite],
      motion: MOTION_BACK[ui.motion],
    },
    locked: view.locked,
  };
}

/** The room's patch as the wire's: only the keys given. */
export function wirePatchOf(patch: Partial<UiSettings>): UiPatch {
  const out: UiPatch = {};
  if (patch.theme !== undefined) out.theme = patch.theme;
  if (patch.font_size !== undefined) out.font_size = patch.font_size;
  if (patch.language !== undefined) out.language = patch.language;
  if (patch.lite !== undefined) out.lite = LITE[patch.lite];
  if (patch.motion !== undefined) out.motion = MOTION[patch.motion];
  return out;
}

/** The room's SettingsHost over the desktop host. One app feed serves every `onSettingsChanged` listener; it is held
 * while any listens. */
export function wireSettingsHost(source: WireSettingsSource): SettingsHost {
  const listeners = new Set<(settings: Settings) => void>();
  let feed: AppFeed | null = null;

  const openFeed = (): void => {
    if (feed !== null || listeners.size === 0) return;
    const opened = source.onAppEvent((event) => {
      if (event.settings_changed === undefined) return;
      const settings = roomSettingsOf(event.settings_changed);
      for (const listener of listeners) listener(settings);
    });
    feed = opened;
    // refused: keep the listener; the transport subscribes again when its channel reopens
    opened.ready.catch(() => {});
  };

  return {
    async getSettings() {
      openFeed();
      // subscribe first, then read: the host answers in order, so no change falls between
      if (feed !== null) await feed.ready.catch(() => {});
      const answer = await source.query("get_settings");
      if (!("settings" in answer) || answer.settings === undefined)
        throw internalError("get_settings answered something else");
      return roomSettingsOf(answer.settings);
    },
    async setUiSettings(patch) {
      await source.dispatch({
        command_id: crypto.randomUUID(),
        command: { set_ui_settings: { patch: wirePatchOf(patch) } },
      });
    },
    onSettingsChanged(listener) {
      listeners.add(listener);
      openFeed();
      return () => {
        listeners.delete(listener);
        if (listeners.size > 0 || feed === null) return;
        feed.stop();
        feed = null;
      };
    },
  };
}
