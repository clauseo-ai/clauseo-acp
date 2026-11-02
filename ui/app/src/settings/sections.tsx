// The settings window's sections, in the design's order (design/03-system/src/pages/settings/screens.ts, SECTIONS).
// A later stage adds its section as one entry here and one file under sections/: its id, its name and one line from
// words-settings.ts, what it says beside its name, whether it shows on this computer now, and its body. A section with
// no entry is hidden, never greyed: a greyed one would promise a date nobody has.
import type { ComponentType } from "react";
import type { Display, Lang } from "@clauseo/components/lib/display.ts";
import { LANGUAGE_NAMES, st, type SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import type { DisplayStore } from "../room/display-store.ts";
import type { UiSettings } from "../room/host-next.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { APP_VERSION } from "./version.ts";
import { lockKey, type LiveSettings } from "./use-settings.ts";
import { About } from "./sections/about.tsx";
import { Appearance } from "./sections/appearance.tsx";
import { Language } from "./sections/language.tsx";

/** Every section the design has, in its order; only those with an entry in SECTIONS show. */
export const SECTION_ORDER = [
  "plans",
  "usage",
  "kanoon",
  "matters",
  "workspace",
  "language",
  "appearance",
  "updates",
  "privacy",
  "about",
] as const;

export type SectionId = (typeof SECTION_ORDER)[number];

/** What a section reads and does: the store, the transport, the settings and locks in force, the display shown, and
 * the lawyer's choice of one setting. */
export type SectionContext = {
  store: DisplayStore;
  transport: HostTransport;
  settings: LiveSettings;
  display: Display;
  /** Whether a firm's policy locks this setting. */
  locked: (key: keyof UiSettings) => boolean;
  /** The lawyer's choice: one patch to the store, unless the key is locked or already holds the value. */
  choose: <K extends keyof UiSettings>(key: K, value: UiSettings[K]) => void;
};

export type SectionEntry = {
  id: SectionId;
  /** The section's name, on the side and as the pane's head. */
  name: SettingsKey;
  /** Optional: the line under the pane's head. */
  lead?: SettingsKey;
  /** Optional: where the section stands now, under its name on the side. */
  note?: (ctx: SectionContext, lang: Lang) => string;
  /** Optional: whether the section shows now; always, when left out. */
  shown?: (ctx: SectionContext) => boolean;
  Body: ComponentType<{ ctx: SectionContext }>;
};

/** The sections built so far, in SECTION_ORDER's order. */
export const SECTIONS: readonly SectionEntry[] = [
  {
    id: "language",
    name: "Language",
    lead: "The language of every word on screen. Papers, quotes and case names stay as filed.",
    note: (ctx) => LANGUAGE_NAMES[ctx.display.lang],
    Body: Language,
  },
  {
    id: "appearance",
    name: "Appearance",
    lead: "How Clauseo looks on this computer.",
    note: (ctx, lang) =>
      st(lang, "{theme}, text size {n}", {
        theme: st(lang, ctx.display.theme === "night" ? "Night" : "Day"),
        n: ctx.display.textSize,
      }),
    Body: Appearance,
  },
  {
    id: "about",
    name: "About",
    note: (_ctx, lang) => st(lang, "Version {version}", { version: APP_VERSION }),
    Body: About,
  },
];

/** A section's context over the store. */
export function contextOf(
  store: DisplayStore,
  transport: HostTransport,
  settings: LiveSettings,
  display: Display,
): SectionContext {
  const locked = (key: keyof UiSettings) => settings.locked.includes(lockKey(key));
  return {
    store,
    transport,
    settings,
    display,
    locked,
    choose: (key, value) => {
      if (locked(key) || settings.ui[key] === value) return;
      store.set({ [key]: value });
    },
  };
}

/** The sections that show now. */
export const shownSections = (ctx: SectionContext): readonly SectionEntry[] =>
  SECTIONS.filter((s) => s.shown?.(ctx) ?? true);

/** The section an address opens at: its hash's (`#appearance`) where that one shows, else the first. */
export function sectionOf(hash: string, shown: readonly SectionEntry[]): SectionId | undefined {
  const id = hash.replace(/^#/, "");
  return (shown.find((s) => s.id === id) ?? shown[0])?.id;
}
