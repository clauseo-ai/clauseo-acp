// The copy review entries for the web sources at the end of an answer (room/thread/sources.tsx), apart from
// registry.tsx. Ids are WEB-01 on, since the registry's id rule (review/status.ts) takes a dash then digits. Each is the real room over FakeTransport: one question, the tool calls that
// searched and read, the answer, and the envelopes that fill those tool calls' web (sources.ts's fold, until the host puts `web` on the entry
// itself). The summary's entries show the list shut; every row's entry opens it, as a press on the summary would.
import { src, type SourcesKey } from "@clauseo/components/lib/words-sources.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import { callOf, eventsOf, searched, visited, webCall } from "../testing/web-sources.ts";
import type { AgentId, Envelope, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver, RoomPressing } from "./states.tsx";
import type { WordsRef } from "./words.ts";

/** A string of words-sources.ts, hi: later while its Hindi is null. */
const sourcesWords = (key: SourcesKey): WordsRef => ({
  file: "lib/words-sources.ts",
  key,
  text: (lang) => src(lang, key),
  later: src.hi[key] === null,
});

const SCREEN = "room/thread";
const QUESTION = userMessage(
  5,
  1,
  "What notice does the Delhi Rent Control Act ask for before eviction?",
);
const ANSWER = agentText(
  40,
  1,
  "Section 14 asks for **notice in writing**, and the courts read it as at least thirty days.",
);

const IK = "https://indiankanoon.org/doc/1755638/";
const DHC =
  "https://delhihighcourt.nic.in/judgments/2019/rent-control-notice-period-eviction-petition.pdf";
const RESULTS = [
  { title: "Section 14 in the Delhi Rent Control Act, 1958", url: IK },
  { title: "Notice to quit under the Rent Control Act", url: "https://www.livelaw.in/rent-notice" },
];

/** The thread's view: the question, a tool call for each one the envelopes name, the answer and its step. */
function entriesFor(envelopes: Envelope[]): ViewEntry[] {
  const calls = [...new Set(envelopes.map(callOf))];
  return [
    QUESTION,
    ...calls.map((call, i) => webCall(10 + i, call, "Web", "fetch")),
    ANSWER,
    step(50),
  ];
}

const hostFor = (envelopes: Envelope[], agent: AgentId) => () => {
  const transport = threadHost({ entries: entriesFor(envelopes), agent });
  // the fake keeps them and plays them to the list as it mounts
  transport.deliver(eventsOf(envelopes));
  return transport;
};

/** The list shut, as an answer first shows it. */
const shut =
  (envelopes: Envelope[], agent: AgentId = "claude-acp") =>
  () => <RoomOver host={hostFor(envelopes, agent)} />;
/** The list opened. */
const opened =
  (envelopes: Envelope[], agent: AgentId = "claude-acp") =>
  () => <RoomPressing host={hostFor(envelopes, agent)} press="web-sources-toggle" />;

const MANY: Envelope[] = [
  searched(1, "Delhi Rent Control Act notice period", RESULTS),
  searched(2, "section 14 eviction notice thirty days"),
  visited(3, IK, { title: "Section 14 in the Delhi Rent Control Act, 1958" }),
  visited(4, DHC),
  visited(5, "https://www.livelaw.in/rent-notice", {
    title: "Notice to quit under the Rent Control Act",
  }),
];

export const WEB_SOURCES: Entry[] = [
  {
    id: "WEB-01",
    words: sourcesWords("{n} web searches."),
    screen: SCREEN,
    render: shut(MANY),
  },
  {
    id: "WEB-02",
    words: sourcesWords("{n} pages read."),
    screen: SCREEN,
    render: shut(MANY),
  },
  {
    id: "WEB-03",
    words: sourcesWords("1 web search."),
    screen: SCREEN,
    render: shut([
      searched(1, "Delhi Rent Control Act notice period", RESULTS),
      visited(2, IK, { title: RESULTS[0]!.title }),
    ]),
  },
  {
    id: "WEB-04",
    words: sourcesWords("1 page read."),
    screen: SCREEN,
    render: shut([visited(1, IK, { title: RESULTS[0]!.title })]),
  },
  {
    id: "WEB-05",
    words: sourcesWords("Searched the web for {query}."),
    screen: SCREEN,
    // Claude's search, its results as links with their sites
    render: opened([searched(1, "Delhi Rent Control Act notice period", RESULTS)]),
  },
  {
    id: "WEB-06",
    words: sourcesWords("No results."),
    screen: SCREEN,
    render: opened([searched(1, "rent control notice Karol Bagh 1987")]),
  },
  {
    id: "WEB-07",
    words: sourcesWords("{name} doesn't report which pages it found."),
    screen: SCREEN,
    render: opened(
      [searched(1, "Delhi Rent Control Act notice period", [], { agent: "codex-acp" })],
      "codex-acp",
    ),
  },
  {
    id: "WEB-08",
    words: sourcesWords("Read {page}."),
    screen: SCREEN,
    // a fetched page with its title, and one with none, whose long address wraps
    render: opened([visited(1, IK, { title: RESULTS[0]!.title }), visited(2, DHC)]),
  },
  {
    id: "WEB-09",
    words: sourcesWords("{name} opened this page on OpenAI's computers, not on this one."),
    screen: SCREEN,
    render: opened(
      [visited(1, IK, { title: RESULTS[0]!.title, via: "codex_open_page" })],
      "codex-acp",
    ),
  },
  {
    id: "WEB-10",
    words: sourcesWords("Opened {page} in the browser."),
    screen: SCREEN,
    render: opened([visited(1, DHC, { title: "Delhi High Court: judgments", via: "browser" })]),
  },
  {
    id: "WEB-11",
    words: sourcesWords("Read {page}."),
    screen: SCREEN,
    // a Hindi query and a Hindi page title, each marked Hindi
    render: opened([
      searched(1, "दिल्ली किराया नियंत्रण अधिनियम बेदखली नोटिस", [
        { title: "दिल्ली किराया नियंत्रण अधिनियम, 1958 की धारा 14", url: IK },
      ]),
      visited(2, IK, { title: "दिल्ली किराया नियंत्रण अधिनियम, 1958 की धारा 14" }),
    ]),
  },
];
