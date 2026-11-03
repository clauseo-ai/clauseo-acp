// The web brief's copy review entries for Settings' Privacy section, apart from registry.tsx to keep that file under the size
// limit: each new string in the real window (settings-app.tsx) over FakeTransport, open at Privacy, at the window's
// size. The plans' rows show both plans while the window knows none, Claude's alone with Claude's limits only; the
// tools' switches show only when the host sends them (app-local today): on, web search off, or both locked and off
// as a firm's policy sends them. The websites' row shows always but with both tools off; the Browser switch's reason
// (no website check) while it is on.
import { tw, type ToolsKey } from "@clauseo/components/lib/words-tools.ts";
import type { ToolSettings } from "../room/host-next.ts";
import type { AgentLimitsView, LimitsView } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { SettingsOver } from "./states-settings.tsx";
import { settingsWords, type WordsRef } from "./words.ts";

/** The settings window, 960 by 700 (DESIGN.md). */
const WINDOW = { width: 960, height: 700 } as const;

/** The state each entry shows Privacy in. */
type State = { tools?: ToolSettings; locked?: string[]; limits?: LimitsView };

const ON: ToolSettings = { web_search: true, browser: true };
const OFF: ToolSettings = { web_search: false, browser: false };

/** A line of the room's tools' words that Settings says too. */
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
  render: (lang) => (
    <SettingsOver
      section="privacy"
      lang={lang}
      {...(state.locked === undefined ? {} : { locked: state.locked })}
      {...(state.limits === undefined ? {} : { limits: state.limits })}
      {...(state.tools === undefined ? {} : { tools: state.tools })}
    />
  ),
});

/** Privacy's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const WEB_SETTINGS: Entry[] = [
  entry("UEP-01", settingsWords("Privacy")),
  entry("UEP-02", settingsWords("What leaves this computer, and who sees it.")),
  entry("UEP-03", settingsWords("What leaves this computer")),
  entry("UEP-04", settingsWords("To {name}, on your plan")),
  entry(
    "UEP-05",
    settingsWords(
      "Each question, the parts of the papers it needs, and its web searches. {name} sees what each question sends it.",
    ),
  ),
  entry("UEP-06", settingsWords("Anthropic lists Brave Search as its web search subprocessor."), {
    limits: CLAUDE_ONLY,
  }),
  entry("UEP-07", settingsWords("On the web"), { tools: ON }),
  entry("UEP-08", settingsWords("Web search"), { tools: { ...ON, web_search: false } }),
  entry(
    "UEP-09",
    settingsWords("Clauseo can search the web and read pages. On unless you turn it off."),
    { tools: ON },
  ),
  entry("UEP-10", settingsWords("Browser"), { tools: ON }),
  entry(
    "UEP-11",
    settingsWords("Clauseo can open web pages in its own browser, on this computer."),
    { tools: ON },
  ),
  entry(
    "UEP-12",
    settingsWords("A change applies to new conversations. Ones already open keep what they had."),
    {
      tools: ON,
    },
  ),
  // the lock's reason in a locked switch's note's place, both locked off by a firm's policy: U11-07's words with the
  // room's period, so a new string under the same id. The ids are UEP-nn: status.ts's form wants digits after the dash
  entry("UEP-13", settingsWords("Managed by your organisation."), {
    tools: OFF,
    locked: ["tools.web_search", "tools.browser"],
  }),
  entry("UEP-14", settingsWords("To the websites Clauseo visits")),
  entry(
    "UEP-15",
    settingsWords(
      "When Clauseo reads a page on this computer or opens one in its browser, the website sees the visit, as it sees any browser's.",
    ),
  ),
  // the Browser switch on, and no website check yet: the room's UET-18 words
  entry("UEP-16", toolWords("Off until Clauseo can check each website the browser opens."), {
    tools: ON,
  }),
];
