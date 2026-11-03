// The settings in force and the keys a firm's policy locks, live: the display store's ui() and locked(), re-read each
// time the store says they changed (a choice here, the host's settings_changed, the other window's write), so the
// controls always show what holds now. The display itself is read with useDisplay().
import { useCallback, useRef, useSyncExternalStore } from "react";
import type { DisplayStore } from "../room/display-store.ts";
import type { UiSettings } from "../room/host-next.ts";
import type { ToolsSettingsView } from "../transport/protocol.ts";

export type LiveSettings = {
  ui: UiSettings;
  locked: readonly string[];
  /** The tools' switches, when the host sends them (SettingsView.tools). */
  tools?: ToolsSettingsView | undefined;
};

const KEYS = ["theme", "font_size", "language", "lite", "motion"] as const;

const sameSettings = (a: LiveSettings, b: LiveSettings) =>
  KEYS.every((k) => a.ui[k] === b.ui[k]) &&
  a.tools?.web_search === b.tools?.web_search &&
  a.tools?.browser === b.tools?.browser &&
  (a.tools === undefined) === (b.tools === undefined) &&
  a.locked.length === b.locked.length &&
  a.locked.every((key, i) => key === b.locked[i]);

/** The store's settings and locks; the same object until either changes, as useSyncExternalStore needs. */
export function useSettings(store: DisplayStore): LiveSettings {
  const last = useRef<LiveSettings | null>(null);
  const subscribe = useCallback((listener: () => void) => store.subscribe(listener), [store]);
  const read = useCallback((): LiveSettings => {
    const next = { ui: store.ui(), locked: store.locked(), tools: store.tools() };
    if (last.current !== null && sameSettings(last.current, next)) return last.current;
    last.current = next;
    return next;
  }, [store]);
  return useSyncExternalStore(subscribe, read, read);
}

/** The dotted key a policy locks a setting by: `ui.theme`. */
export const lockKey = (key: keyof UiSettings) => `ui.${key}`;
