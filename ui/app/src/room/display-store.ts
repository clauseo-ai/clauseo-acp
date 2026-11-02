// The room's display settings, and the display they show on <html> (ui/components' display.ts). The settings are H7's
// `ui` table (host-next.ts). Until H7, they live in localStorage under "clauseo.display", in that shape; with a
// NextHost the host holds them: read at start, written as patches, followed as they change. Theme and motion follow the
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

/** Where the settings live until H7. */
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
  /** Stops following the computer's preferences and the host. */
  dispose(): void;
}

const DARK = "(prefers-color-scheme: dark)";
const REDUCED = "(prefers-reduced-motion: reduce)";

function readStored(storage: DisplayEnv["storage"]): UiSettings {
  try {
    const text = storage?.getItem(DISPLAY_KEY) ?? null;
    return { ...DEFAULT_UI, ...(text === null ? {} : uiPatchOf(JSON.parse(text))) };
  } catch {
    // unreadable or not JSON: the defaults, and the next change writes over it
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

  const host = env.host;
  let locked: readonly string[] = [];
  let stopHost = () => {};
  const fromHost = (settings: Settings) => {
    stored = { ...DEFAULT_UI, ...uiPatchOf(settings.ui) };
    locked = settings.locked;
    apply();
  };
  const readHost = (current: SettingsHost) =>
    current
      .getSettings()
      .then(fromHost, (error: unknown) =>
        console.warn(`get_settings failed: ${JSON.stringify(error)}`),
      );
  if (host !== null) {
    stopHost = host.onSettingsChanged(fromHost);
    void readHost(host);
  }

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
      apply();
    },
    override(patch) {
      over = { ...over, ...uiPatchOf(patch) };
      apply();
    },
    setPlatform(next) {
      platform = next;
      apply();
    },
    batch: () => (current.render === "lite" ? liteBatch : frameBatch),
    dispose() {
      dark?.removeEventListener("change", apply);
      reduced?.removeEventListener("change", apply);
      stopHost();
    },
  };
}
