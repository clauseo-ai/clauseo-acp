// The web brief's copy review entries for Settings' Privacy section, apart from registry.tsx to keep that file under the size
// limit: each new string in the real window (settings-app.tsx) over FakeTransport, open at Privacy, at the window's
// size. The plans' rows show both plans while the window knows none, Claude's alone with Claude's limits only; the
// tools' switches show only when the host sends them (app-local today): on, web search off, or both locked and off
// as a firm's policy sends them. The websites' row shows always but with both tools off; the Browser switch's reason
// (no website check) while it is on. An info tip's entries show the tip open, pressed once the page has it.
import { st, type SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import { tw, type ToolsKey } from "@clauseo/components/lib/words-tools.ts";
import { toolsLine, type ToolsLineKey } from "@clauseo/components/lib/words-tools-line.ts";
import type { ToolSettings } from "../room/host-next.ts";
import type { AgentLimitsView, LimitsView } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { OpenTip } from "./open-tip.tsx";
import { SettingsOver } from "./states-settings.tsx";
import { settingsWords, type WordsRef } from "./words.ts";

/** The settings window, 960 by 700 (DESIGN.md). */
const WINDOW = { width: 960, height: 700 } as const;

/** The state each entry shows Privacy in; `tip` names an info tip's button to press open once it shows. */
type State = { tools?: ToolSettings; locked?: string[]; limits?: LimitsView; tip?: SettingsKey };

const ON: ToolSettings = { web_search: true, browser: true };
const OFF: ToolSettings = { web_search: false, browser: false };

/** A line of the room's tool line words (words-tools-line.ts) that Settings says too. */
const lineWords = (key: ToolsLineKey): WordsRef => ({
  file: "lib/words-tools-line.ts",
  key,
  text: (lang) => toolsLine(lang, key),
  later: toolsLine.hi[key] === null,
});

/** A line of the room's popover words (words-tools.ts) that Settings says too. */
const toolWords = (key: ToolsKey): WordsRef => ({
  file: "lib/words-tools.ts",
  key,
  text: (lang) => tw(lang, key),
  later: tw.hi[key] === null,
});

/** Claude's plan alone, so the rows name Claude only. */
const CLAUDE: AgentLimitsView = {
  plan: "Claude Max",
  billing: "plan",
  windows: [],
  at_limit_text: "",
  freshness: "unavailable",
};
const CLAUDE_ONLY: LimitsView = { claude: CLAUDE };

const entry = (id: string, words: WordsRef, state: State = {}): Entry => ({
  id,
  words,
  screen: "settings/privacy",
  ...WINDOW,
  render: (lang) => {
    const over = (
      <SettingsOver
        section="privacy"
        lang={lang}
        {...(state.locked === undefined ? {} : { locked: state.locked })}
        {...(state.limits === undefined ? {} : { limits: state.limits })}
        {...(state.tools === undefined ? {} : { tools: state.tools })}
      />
    );
    return state.tip === undefined ? over : <OpenTip label={st(lang, state.tip)}>{over}</OpenTip>;
  },
});

/** Privacy's entries, each id written as it is: an id never moves when an entry is added or taken out. Retired in
 * round 3 (icons first): UEP-05, UEP-09, UEP-11, UEP-12. */
export const WEB_SETTINGS: Entry[] = [
  entry("UEP-01", settingsWords("Privacy")),
  entry("UEP-02", settingsWords("What leaves this computer, and who sees it.")),
  entry("UEP-03", settingsWords("What leaves this computer")),
  entry("UEP-04", settingsWords("To {name}, on your plan")),
  entry(
    "UEP-17",
    settingsWords("Each question, the parts of the papers it needs, and its web searches."),
  ),
  // behind Claude's info icon, the tip open
  entry("UEP-06", settingsWords("Anthropic lists Brave Search as its web search subprocessor."), {
    limits: CLAUDE_ONLY,
    tip: "More about Brave Search",
  }),
  entry("UEP-21", settingsWords("More about Brave Search"), {
    limits: CLAUDE_ONLY,
    tip: "More about Brave Search",
  }),
  // the group as a firm locks it: both switches off, each glyph locked, each with U11-07's approved "Managed by your
  // organisation" in its helper's place (that line is U11-07's, approved, so it takes no id here)
  entry("UEP-07", settingsWords("On the web"), {
    tools: OFF,
    locked: ["tools.web_search", "tools.browser"],
  }),
  // web search off: its globe slashed
  entry("UEP-08", settingsWords("Web search"), { tools: { ...ON, web_search: false } }),
  entry("UEP-18", settingsWords("Search the web and read pages."), { tools: ON }),
  // the browser off: its window slashed, its helper shown (the no-check reason stands only while it is on)
  entry("UEP-10", settingsWords("Browser"), { tools: { ...ON, browser: false } }),
  entry("UEP-19", settingsWords("Clauseo's own browser, on this computer."), {
    tools: { ...ON, browser: false },
  }),
  // the group's note, the room's words (UET's popover tooltip says them too)
  entry("UEP-20", toolWords("Changes apply to new conversations."), { tools: ON }),
  entry("UEP-14", settingsWords("To the websites Clauseo visits")),
  entry(
    "UEP-15",
    settingsWords(
      "When Clauseo reads a page on this computer or opens one in its browser, the website sees the visit, as it sees any browser's.",
    ),
    { tip: "More about the websites Clauseo visits" },
  ),
  entry("UEP-22", settingsWords("More about the websites Clauseo visits"), {
    tip: "More about the websites Clauseo visits",
  }),
  // the Browser switch on, and no website check yet: the room's reason in its helper's place
  entry("UEP-16", lineWords("Not available yet"), { tools: ON }),
];
