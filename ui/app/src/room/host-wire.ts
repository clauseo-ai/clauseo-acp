// H7, H8, R15 and H3 to H6 over the desktop host: the room's SettingsHost (host-next.ts) from the transport's `query`,
// `dispatch` and app feed (`onAppEvent`), its PapersHost from `query`'s `list_papers`, its AgentsHost from `query`'s
// `list_agents`, its MattersHost from `list_matters`, `list_threads` and the matters' commands, and its AppHost from
// the app feed's `matters_changed` and `threads_changed`. The room keeps its own spellings; this file is the one place
// they meet the wire's: lite's "full" and "lite" are the wire's "off" and "on", motion's "reduced" is "reduce", the text
// size is a number on the wire, and a matter's details the wire leaves out read as its binding says (`roomMatterOf`).
import { DEFAULT_DISPLAY, TEXT_SIZES, type TextSize } from "@clauseo/components/lib/display.ts";
import {
  internalError,
  type AppFeed,
  type AppFeedSource,
  type HostTransport,
} from "../transport/host-transport.ts";
import type {
  CommandOutcome,
  HostEvent,
  Lite,
  MatterSummary,
  Motion,
  SettingsView,
  UiPatch,
  UiSettings as WireUi,
} from "../transport/protocol.ts";
import type {
  AgentsHost,
  AppEvent,
  AppHost,
  MatterInfo,
  MattersCommand,
  MattersHost,
  NextOutcome,
  Papers,
  PapersHost,
  Settings,
  SettingsHost,
  UiLite,
  UiMotion,
  UiSettings,
} from "./host-next.ts";

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

/** The room's PapersHost over the desktop host: `{"list_papers": {"matter": ...}}`, answered with `papers`. */
export function wirePapersHost(source: Pick<HostTransport, "query">): PapersHost {
  return {
    async listPapers(matter): Promise<Papers> {
      const answer = await source.query({ list_papers: { matter } });
      if (!("papers" in answer) || answer.papers === undefined)
        throw internalError("list_papers answered something else");
      return answer.papers;
    },
  };
}

/** The room's AgentsHost over the desktop host: `"list_agents"`, answered with `agents`. */
export function wireAgentsHost(source: Pick<HostTransport, "query">): AgentsHost {
  return {
    async listAgents() {
      const answer = await source.query("list_agents");
      if (!("agents" in answer) || answer.agents === undefined)
        throw internalError("list_agents answered something else");
      return answer.agents;
    },
  };
}

/** A matter row as the room reads it: each detail the wire may leave out as its binding says a missing one reads
 * (unpinned, idle, its folder found); a folder state from a newer host that this one doesn't know reads as found too,
 * so the row offers no Locate folder it can't explain. */
export function roomMatterOf(m: MatterSummary): MatterInfo {
  const { pinned, status, folder_state, ...rest } = m;
  return {
    ...rest,
    pinned: pinned ?? false,
    status: status ?? "idle",
    folder_state: folder_state === "missing" ? "missing" : "found",
  };
}

/** A matters command's name: its one key. */
const commandName = (command: MattersCommand): string => Object.keys(command)[0] ?? "command";

/** A matters command's outcome as the room's: create_matter's `matter_created`, every other's `done`. */
function outcomeOf(name: string, outcome: CommandOutcome): NextOutcome {
  if (name === "create_matter") {
    if (typeof outcome === "object" && outcome.matter_created !== undefined)
      return { matter_created: outcome.matter_created };
  } else if (outcome === "done") return outcome;
  throw internalError(`${name} answered something else`);
}

/** The room's MattersHost over the desktop host: `"list_matters"` answered with `matters`, `{"list_threads": {"matter":
 * ...}}` with `threads`, and each matters command through `dispatch` with a fresh id. */
export function wireMattersHost(source: Pick<HostTransport, "query" | "dispatch">): MattersHost {
  return {
    async listMatters() {
      const answer = await source.query("list_matters");
      if (!("matters" in answer) || answer.matters === undefined)
        throw internalError("list_matters answered something else");
      return answer.matters.map(roomMatterOf);
    },
    async listThreads(matter) {
      const answer = await source.query({ list_threads: { matter } });
      if (!("threads" in answer) || answer.threads === undefined)
        throw internalError("list_threads answered something else");
      return answer.threads;
    },
    async command(command) {
      const outcome = await source.dispatch({ command_id: crypto.randomUUID(), command });
      return outcomeOf(commandName(command), outcome);
    },
  };
}

/** The app feed's event as the room's AppEvent: `matters_changed` with its rows read as the room reads them,
 * `threads_changed` as it came; null for the rest (settings_changed is wireSettingsHost's). */
function appEventOf(event: HostEvent): AppEvent | null {
  if (event.matters_changed !== undefined) {
    const { matters, removed, all } = event.matters_changed;
    return { matters_changed: { matters: matters.map(roomMatterOf), removed, all } };
  }
  if (event.threads_changed !== undefined) return { threads_changed: event.threads_changed };
  return null;
}

/** The room's AppHost over the desktop host's app feed. One feed serves every `subscribeApp` listener; it is held while
 * any listens. The host's first frames on a subscription are the whole lists (`all: true`), so no change falls before
 * the first read. */
export function wireAppHost(source: AppFeedSource): AppHost {
  const listeners = new Set<(event: AppEvent) => void>();
  let feed: AppFeed | null = null;
  return {
    subscribeApp(listener) {
      listeners.add(listener);
      if (feed === null) {
        const opened = source.onAppEvent((wire) => {
          const event = appEventOf(wire);
          if (event === null) return;
          for (const heard of listeners) heard(event);
        });
        feed = opened;
        // refused: keep the listener; the transport subscribes again when its channel reopens
        opened.ready.catch(() => {});
      }
      return () => {
        listeners.delete(listener);
        if (listeners.size > 0 || feed === null) return;
        feed.stop();
        feed = null;
      };
    },
  };
}
