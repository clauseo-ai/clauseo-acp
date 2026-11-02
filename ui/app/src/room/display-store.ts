// The room's display settings, and the display they show on <html> (ui/components' display.ts). The settings are H7's
// `ui` table (host-next.ts). Without a host they live in localStorage under "clauseo.display", in that shape; with
// one the host holds them: read at start, written as patches, followed as they change, and localStorage keeps its last
// answer for the next start's first frame. Without a host, another window's write to localStorage (the settings window's,
// the room's) applies here at once, through the `storage` event. Theme and motion follow the
// computer's light and reduced-motion preferences, live, until set; lite's "automatic" means off until H7's display
// report; English and text size 16 until set. The platform is the host's (platform.ts), never a setting.
import {
  DEFAULT_DISPLAY,
  TEXT_SIZES,
  type Display,
  type Platform,
} from "@clauseo/components/lib/display.ts";
import { frameBatch, liteBatch, type Batch } from "../store/batch.ts";
import type { Settings, SettingsHost, UiSettings } from "./host-next.ts";

/** The settings without a host; with one, a cache of the host's last answer, read only for the first frame. */
export const DISPLAY_KEY = "clauseo.display";

/** The settings before the lawyer sets any. */
export const DEFAULT_UI: UiSettings = {
  theme: "system",
  font_size: DEFAULT_DISPLAY.textSize,
  language: "en",
  lite: "automatic",
  motion: "system",
};

const one = <T extends string | number>(choices: readonly T[], value: unknown): T | undefined =>
  choices.find((choice) => choice === value);

/** The keys of `value` that are valid settings, each checked; anything else is dropped. */
export function uiPatchOf(value: unknown): Partial<UiSettings> {
  if (typeof value !== "object" || value === null) return {};
  const v = value as Record<string, unknown>;
  const patch: Partial<UiSettings> = {};
  const theme = one(["system", "light", "dark"] as const, v["theme"]);
  const size = one(TEXT_SIZES, v["font_size"]);
  const language = one(["en", "hi"] as const, v["language"]);
  const lite = one(["automatic", "full", "lite"] as const, v["lite"]);
  const motion = one(["system", "full", "reduced"] as const, v["motion"]);
  if (theme !== undefined) patch.theme = theme;
  if (size !== undefined) patch.font_size = size;
  if (language !== undefined) patch.language = language;
  if (lite !== undefined) patch.lite = lite;
  if (motion !== undefined) patch.motion = motion;
  return patch;
}

/** The computer's side of the display: its light and motion preferences, and its OS. */
export type Surroundings = { dark: boolean; reduced: boolean; platform: Platform };

/** The display the settings show on this computer now. */
export function displayOf(ui: UiSettings, around: Surroundings): Display {
  return {
    theme: ui.theme === "dark" || (ui.theme === "system" && around.dark) ? "night" : "day",
    // "automatic" waits on H7's display report, so it is off until then
    render: ui.lite === "lite" ? "lite" : "full",
    motion:
      ui.motion === "reduced" || (ui.motion === "system" && around.reduced) ? "reduced" : "full",
    lang: ui.language,
    textSize: ui.font_size,
    platform: around.platform,
  };
}

const same = (a: Display, b: Display) =>
  a.theme === b.theme &&
  a.render === b.render &&
  a.motion === b.motion &&
  a.lang === b.lang &&
  a.textSize === b.textSize &&
  a.platform === b.platform;

/** What the store reaches: each null where the page has none (a test, a page outside a browser). */
export type DisplayEnv = {
  storage: Pick<Storage, "getItem" | "setItem"> | null;
  matchMedia: ((query: string) => MediaQueryList) | null;
  /** Where another window's change to localStorage arrives as a `storage` event: the page's window. Heard only without
   * a host, whose settings_changed reaches every window. Null or left out, no other window is heard. */
  events?: Pick<EventTarget, "addEventListener" | "removeEventListener"> | null;
  /** H7's part of the NextHost (host-next.ts), or null where the host lacks it. */
  host: SettingsHost | null;
  /** Shows the first display, before the first render: display.ts's startDisplay. */
  start: (display: Display) => void;
  /** Moves to a later one: display.ts's showDisplay. */
  show: (display: Display) => void;
  /** Settings for this page only, never written: a dev or probe page's query. */
  override?: Partial<UiSettings>;
  /** The OS to lay out for until the host says (setPlatform): the webview's own guess (platform.ts's guessPlatform),
   * so a Windows window doesn't start in the Mac's places and move. The Mac where left out. */
  platform?: Platform;
};

export interface DisplayStore {
  /** The settings in force: the stored or the host's, with this page's override over them. */
  ui(): UiSettings;
  /** The keys the firm's policy locks, dotted (`ui.theme`), as the host last said; none without a host. */
  locked(): readonly string[];
  display(): Display;
  /** The lawyer's change: written to localStorage, or with a NextHost sent to the host as a patch. */
  set(patch: Partial<UiSettings>): void;
  /** Settings for this page only, never written; a later `set` of the same key takes over from it. */
  override(patch: Partial<UiSettings>): void;
  /** The host's OS, once it says. */
  setPlatform(platform: Platform): void;
  /** When the view store applies what arrived: every 100 ms in lite, else once a frame. */
  batch(): Batch;
  /** Hears each change to `ui()` or `locked()`: a set, an override, the host's answer, another window's write. Not the
   * display alone (the computer's preferences, the platform): read that with display.ts's useDisplay. Returns the
   * unsubscribe. */
  subscribe(listener: () => void): () => void;
  /** Stops following the computer's preferences, the host and the other windows. */
  dispose(): void;
}

const DARK = "(prefers-color-scheme: dark)";
const REDUCED = "(prefers-reduced-motion: reduce)";

/** The settings a stored text holds over the defaults; unreadable, or not JSON, the defaults. */
function uiOfText(text: string | null): UiSettings {
  try {
    return { ...DEFAULT_UI, ...(text === null ? {} : uiPatchOf(JSON.parse(text))) };
  } catch {
    // not JSON: the defaults, and the next change writes over it
    return { ...DEFAULT_UI };
  }
}

function readStored(storage: DisplayEnv["storage"]): UiSettings {
  try {
    return uiOfText(storage?.getItem(DISPLAY_KEY) ?? null);
  } catch {
    // unreadable: the defaults, and the next change writes over it
    return { ...DEFAULT_UI };
  }
}

/** Starts the display from the settings, at once (display.ts's startDisplay), and keeps it in step with them. */
export function createDisplayStore(env: DisplayEnv): DisplayStore {
  let stored = readStored(env.storage);
  let over: Partial<UiSettings> = uiPatchOf(env.override);
  let platform: Platform = env.platform ?? DEFAULT_DISPLAY.platform;
  const dark = env.matchMedia?.(DARK) ?? null;
  const reduced = env.matchMedia?.(REDUCED) ?? null;
  const ui = (): UiSettings => ({ ...stored, ...over });
  const compute = () =>
    displayOf(ui(), {
      dark: dark?.matches ?? false,
      reduced: reduced?.matches ?? false,
      platform,
    });
  let current = compute();
  env.start(current);
  const apply = () => {
    const next = compute();
    if (same(next, current)) return;
    current = next;
    env.show(next);
  };
  dark?.addEventListener("change", apply);
  reduced?.addEventListener("change", apply);
  const listeners = new Set<() => void>();
  /** Shows the settings' display and tells the listeners the settings changed. */
  const changed = () => {
    apply();
    for (const listener of listeners) listener();
  };

  const host = env.host;
  let locked: readonly string[] = [];
  let stopHost = () => {};
  /** The settings_changed events heard: a read that one overtook is older than it, and dropped. */
  let heard = 0;
  const fromHost = (settings: Settings) => {
    stored = { ...DEFAULT_UI, ...uiPatchOf(settings.ui) };
    locked = settings.locked;
    try {
      env.storage?.setItem(DISPLAY_KEY, JSON.stringify(stored));
    } catch {
      // only the next start's first frame reads it
    }
    changed();
  };
  // the wire host subscribes before it reads, so a change after the read also arrives as an event
  const readHost = (current: SettingsHost) => {
    const at = heard;
    return current.getSettings().then(
      (settings) => {
        if (heard === at) fromHost(settings);
      },
      (error: unknown) => console.warn(`get_settings failed: ${JSON.stringify(error)}`),
    );
  };
  if (host !== null) {
    stopHost = host.onSettingsChanged((settings) => {
      heard += 1;
      fromHost(settings);
    });
    void readHost(host);
  }
  // Without a host the windows share only localStorage: another window's write arrives here as a storage event (a
  // window never hears its own). With one, the host's settings_changed already reaches every window, and the host's
  // answer written to localStorage as the next start's cache must not apply a second time.
  const events = host === null ? (env.events ?? null) : null;
  const onStorage = (event: Event) => {
    const { key, newValue } = event as StorageEvent;
    // null: the whole storage was cleared
    if (key !== null && key !== DISPLAY_KEY) return;
    stored = uiOfText(key === null ? null : newValue);
    changed();
  };
  events?.addEventListener("storage", onStorage);

  return {
    ui,
    locked: () => locked,
    display: () => current,
    set(patch) {
      const clean = uiPatchOf(patch);
      stored = { ...stored, ...clean };
      const rest = { ...over };
      for (const key of Object.keys(clean) as (keyof UiSettings)[]) delete rest[key];
      over = rest;
      if (host !== null) {
        host.setUiSettings(clean).catch((error: unknown) => {
          console.warn(`set_ui_settings failed: ${JSON.stringify(error)}`);
          // the host refused it (a locked key, say): show what the host holds, not the refused value
          void readHost(host);
        });
      } else {
        try {
          env.storage?.setItem(DISPLAY_KEY, JSON.stringify(stored));
        } catch (error) {
          // storage full or refused: the change still shows on this page
          console.warn(`clauseo.display not saved: ${String(error)}`);
        }
      }
      changed();
    },
    override(patch) {
      over = { ...over, ...uiPatchOf(patch) };
      changed();
    },
    setPlatform(next) {
      platform = next;
      apply();
    },
    batch: () => (current.render === "lite" ? liteBatch : frameBatch),
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    dispose() {
      dark?.removeEventListener("change", apply);
      reduced?.removeEventListener("change", apply);
      events?.removeEventListener("storage", onStorage);
      listeners.clear();
      stopHost();
    },
  };
}
