// The settings window's sections, in the design's order (design/03-system/src/pages/settings/screens.ts, SECTIONS).
// A later stage adds its section as one entry here and one file under sections/: its id, its name and one line from
// words-settings.ts, what it says beside its name, whether it shows on this computer now, and its body. A section with
// no entry is hidden, never greyed: a greyed one would promise a date nobody has.
import type { ComponentType } from "react";
import type { IconProps } from "@clauseo/components/icons/icon.tsx";
import {
  IconAbout,
  IconAppearance,
  IconCommand,
  IconConversation,
  IconLanguage,
  IconPlanLimit,
  IconSafeWorkspace,
  IconSearch,
  IconSend,
  IconTryAgain,
} from "@clauseo/components/icons/icons.tsx";
import type { Display, Lang } from "@clauseo/components/lib/display.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { mw } from "@clauseo/components/lib/words-mcp-tools.ts";
import { LANGUAGE_NAMES, st, type SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import { uw } from "@clauseo/components/lib/words-update.ts";
import type { DisplayStore } from "../room/display-store.ts";
import type { UiSettings } from "../room/host-next.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { NO_CONNECTORS } from "../store/connectors.ts";
import type {
  ConnectorsState,
  LimitsView,
  ToolsPatch,
  ToolsSettingsView,
} from "../transport/protocol.ts";
import { APP_VERSION } from "./version.ts";
import type { NotSetUp } from "./plan-setup.ts";
import { lockKey, type LiveSettings } from "./use-settings.ts";
import { hashId } from "./use-hash-section.ts";
import { About } from "./sections/about.tsx";
import { Appearance } from "./sections/appearance.tsx";
import { Language } from "./sections/language.tsx";
import { Privacy } from "./sections/privacy.tsx";
import { Usage } from "./sections/usage.tsx";
import { Updates } from "./sections/updates.tsx";
import { carriesUpdate } from "../store/update.ts";
import { ConnectorsSection } from "./sections/connectors-load.tsx";
import { PlansSection } from "./sections/plans-load.tsx";
import { carriesSetup } from "./sections/plans-shown.ts";
import { ToolsSection } from "./sections/mcp-tools-load.tsx";
import { toolsCommandsOf } from "./sections/tools-model.ts";
import { WorkspaceSection } from "./sections/workspace-load.tsx";
import { carriesProtectionWire } from "./sections/workspace-shown.ts";
import { connectorsNote } from "./sections/connectors-note.ts";
import { usageNote } from "./sections/usage-note.ts";

/** Every section the design has, in its order; only those with an entry in SECTIONS show. */
export const SECTION_ORDER = [
  "plans",
  "usage",
  "connectors",
  "tools",
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
  /** Every agent's limits, from the window's limits store: Plan and usage's note reads them. */
  limits: LimitsView;
  /** Whether the setup wire says a plan isn't signed in (plan-setup.ts): Plan and usage shows no billing for it. */
  notSetUp: NotSetUp;
  /** The release date the host reports (`YYYY-MM-DD`), or null: About's line reads it. */
  releaseDate: string | null;
  /** Whether a firm's policy locks this setting. */
  locked: (key: keyof UiSettings) => boolean;
  /** The lawyer's choice: one patch to the store, unless the key is locked or already holds the value. */
  choose: <K extends keyof UiSettings>(key: K, value: UiSettings[K]) => void;
  /** Whether a firm's policy locks a tool's switch (`tools.web_search`, `tools.browser`). */
  toolLocked: (key: keyof ToolsSettingsView) => boolean;
  /** The connectors' state from the window's connectors store (store/connectors.ts): Connectors reads it, and shows
   * only while it lists a connector. */
  connectors: ConnectorsState;
  /** The lawyer's choice of a tool's switch: one patch, unless the key is locked or already holds the value. The
   * connectors' switch (`connectors`, U15) reads both from the connectors' state, since ToolsSettingsView has none. */
  chooseTool: (key: keyof ToolsPatch, value: boolean) => void;
  /** Whether a firm's policy locks the anonymous counts' switch (`privacy.counts`). */
  countsLocked: () => boolean;
  /** The lawyer's choice of the anonymous counts' switch: one patch, unless locked or already holding the value. */
  chooseCounts: (value: boolean) => void;
};

export type SectionEntry = {
  id: SectionId;
  /** The section's name, on the side and as the pane's head: a settings string, or a later section's own words. */
  name: SettingsKey | ((lang: Lang) => string);
  /** Its icon, before the name on the side. */
  icon: ComponentType<IconProps>;
  /** Optional: the line under the pane's head. */
  lead?: SettingsKey | ((lang: Lang) => string);
  /** Optional: where the section stands now, under its name on the side; nothing when it gives undefined. */
  note?: (ctx: SectionContext, lang: Lang) => string | undefined;
  /** Optional: whether the section shows now; always, when left out. */
  shown?: (ctx: SectionContext) => boolean;
  Body: ComponentType<{ ctx: SectionContext }>;
};

/** The sections built so far, in SECTION_ORDER's order. */
export const SECTIONS: readonly SectionEntry[] = [
  {
    // first, as the design has it, so the window opens on Plans; no side note, so the setup store stays out of the
    // first load. Only where the transport carries the setup wire (UD2)
    id: "plans",
    name: "Plans",
    icon: IconConversation,
    lead: "The plans Clauseo works with. Each signs in through its own page in your browser.",
    shown: (ctx) => carriesSetup(ctx.transport),
    Body: PlansSection,
  },
  {
    id: "usage",
    name: "Plan and usage",
    icon: IconPlanLimit,
    lead: "What each plan has left, and when it resets.",
    note: (ctx, lang) => usageNote(ctx.limits, lang, ctx.notSetUp),
    Body: Usage,
  },
  {
    id: "connectors",
    name: (lang) => cw(lang, "Connectors"),
    icon: IconSearch,
    lead: (lang) => cw(lang, "Paid sources Clauseo can search while it answers."),
    note: (ctx, lang) => connectorsNote(ctx.connectors, lang),
    shown: (ctx) => ctx.connectors.connectors.length > 0,
    Body: ConnectorsSection,
  },
  {
    // UC-169 to UC-189: the lawyer's own MCP servers, where the transport carries their commands (tools-model.ts); the
    // lead is the plain line on what a tool can reach. No side note, so the list stays out of the first load
    id: "tools",
    name: (lang) => mw(lang, "Tools"),
    icon: IconCommand,
    lead: (lang) =>
      mw(
        lang,
        "A tool runs on this computer. It can reach anything your account here can: your files, your programs and the internet.",
      ),
    shown: (ctx) => toolsCommandsOf(ctx.transport) !== null,
    Body: ToolsSection,
  },
  {
    // UC: this computer's protection, where the transport carries its wire; built now, live once R17 PR-B admits
    // this window (Ruled, 7). No side note, so the protection store stays out of the first load
    id: "workspace",
    name: "Safe workspace",
    icon: IconSafeWorkspace,
    shown: (ctx) => carriesProtectionWire(ctx.transport),
    Body: WorkspaceSection,
  },
  {
    id: "language",
    name: "Language",
    icon: IconLanguage,
    lead: "The language of every word on screen. Papers, quotes and case names stay as filed.",
    note: (ctx) => LANGUAGE_NAMES[ctx.display.lang],
    Body: Language,
  },
  {
    id: "appearance",
    name: "Appearance",
    icon: IconAppearance,
    lead: "How Clauseo looks on this computer.",
    // the Theme control's choice, in its words: Follow the computer, not the light it gives now (D-067)
    note: (ctx, lang) =>
      st(lang, "{theme}, text size {n}", {
        theme:
          ctx.settings.ui.theme === "system"
            ? frame(lang, "Follow the computer")
            : st(lang, ctx.settings.ui.theme === "dark" ? "Night" : "Day"),
        n: ctx.display.textSize,
      }),
    Body: Appearance,
  },
  {
    // R23: where the transport carries the updater; no side note, so nothing waits on the updater's first answer
    id: "updates",
    name: (lang) => uw(lang, "Updates"),
    icon: IconTryAgain,
    shown: (ctx) => carriesUpdate(ctx.transport),
    Body: Updates,
  },
  {
    id: "privacy",
    name: "Privacy",
    icon: IconSend,
    // no lead: the pane's first heading, "What leaves this computer", says it, and a lead would repeat it
    Body: Privacy,
  },
  {
    id: "about",
    name: "About",
    icon: IconAbout,
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
  limits: LimitsView = {},
  releaseDate: string | null = null,
  connectors: ConnectorsState = NO_CONNECTORS,
  notSetUp: NotSetUp = () => false,
): SectionContext {
  const locked = (key: keyof UiSettings) => settings.locked.includes(lockKey(key));
  const toolLocked = (key: keyof ToolsSettingsView) => settings.locked.includes(`tools.${key}`);
  const countsLocked = () => settings.locked.includes("privacy.counts");
  return {
    store,
    transport,
    settings,
    display,
    limits,
    notSetUp,
    releaseDate,
    connectors,
    locked,
    choose: (key, value) => {
      if (locked(key) || settings.ui[key] === value) return;
      store.set({ [key]: value });
    },
    toolLocked,
    chooseTool: (key, value) => {
      if (key === "connectors") {
        if (connectors.locked || connectors.available === value) return;
        store.setTools({ connectors: value });
        return;
      }
      if (toolLocked(key) || settings.tools === undefined || settings.tools[key] === value) return;
      store.setTools({ [key]: value });
    },
    countsLocked,
    chooseCounts: (value) => {
      if (countsLocked() || settings.privacy === undefined || settings.privacy.counts === value)
        return;
      store.setPrivacy({ counts: value });
    },
  };
}

/** The sections that show now. */
export const shownSections = (ctx: SectionContext): readonly SectionEntry[] =>
  SECTIONS.filter((s) => s.shown?.(ctx) ?? true);

/** The section an address opens at: its hash's (`#appearance`, `#protection` for the Safe workspace) where that one
 * shows, else the first. */
export function sectionOf(hash: string, shown: readonly SectionEntry[]): SectionId | undefined {
  const id = hashId(hash);
  return (shown.find((s) => s.id === id) ?? shown[0])?.id;
}
