// The copy review entries for the web sources at the end of an answer (room/thread/sources.tsx), apart from
// registry.tsx. Ids are WEB-01 on, since the registry's id rule (review/status.ts) takes a dash then digits. Each is the real room over FakeTransport: one question, the tool calls that
// searched and read, the answer, and the envelopes that fill those tool calls' web (sources.ts's fold, until the host puts `web` on the entry
// itself). WEB-12 shows the list shut, as an answer first does; every other entry shows it open from the start
// (SourcesOpenAtFirst), so its row's words are there without a click.
import { src, type SourcesKey } from "@clauseo/components/lib/words-sources.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import { eventsOf, searched, visited, webCallsFor } from "../testing/web-sources.ts";
import type { AgentId, Envelope, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { SourcesOpenAtFirst } from "../room/thread/sources.ts";
import { RoomOver } from "./states.tsx";
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

const ACT = "https://www.indiacode.nic.in/handle/123456789/1525/simple-search?query=section%2014";
const DHC =
  "https://delhihighcourt.nic.in/judgments/2019/rent-control-notice-period-eviction-petition.pdf";
const RESULTS = [
  { title: "Section 14 in the Delhi Rent Control Act, 1958", url: ACT },
  { title: "Notice to quit under the Rent Control Act", url: "https://www.livelaw.in/rent-notice" },
];

/** The thread's view: the question, a tool call for each one the envelopes name, titled as the agent's adapter
 * titles it, the answer and its step. */
const entriesFor = (envelopes: Envelope[], agent: AgentId): ViewEntry[] => [
  QUESTION,
  ...webCallsFor(envelopes, agent),
  ANSWER,
  step(50),
];

const hostFor = (envelopes: Envelope[], agent: AgentId) => () => {
  const transport = threadHost({ entries: entriesFor(envelopes, agent), agent });
  // the fake keeps them and plays them to the list as it mounts
  transport.deliver(eventsOf(envelopes));
  return transport;
};

/** The list shut, as an answer first shows it. */
const shut =
  (envelopes: Envelope[], agent: AgentId = "claude-acp") =>
  () => <RoomOver host={hostFor(envelopes, agent)} />;
/** The list open from the start, as the review shows a row's words: the room's own default stays shut. */
const opened =
  (envelopes: Envelope[], agent: AgentId = "claude-acp") =>
  () => (
    <SourcesOpenAtFirst value>
      <RoomOver host={hostFor(envelopes, agent)} />
    </SourcesOpenAtFirst>
  );

const MANY: Envelope[] = [
  searched(1, "Delhi Rent Control Act notice period", RESULTS),
  searched(2, "section 14 eviction notice thirty days"),
  visited(3, ACT, { title: "Section 14 in the Delhi Rent Control Act, 1958" }),
  visited(4, DHC),
  visited(5, "https://www.livelaw.in/rent-notice", {
    title: "Notice to quit under the Rent Control Act",
  }),
];

// WEB-01 to WEB-04 named the old summary's two sentences ("2 web searches. 3 pages read."), which repeated the step
// chip's counts; they are retired, not reused. The summary's one line is WEB-12 and WEB-13.
export const WEB_SOURCES: Entry[] = [
  {
    id: "WEB-05",
    words: sourcesWords("Searched online for '{query}'."),
    screen: SCREEN,
    // the whole list open: Claude's search with its results as links and their sites, a search with none, and pages
    render: opened(MANY),
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
    words: sourcesWords("Read {page} online."),
    screen: SCREEN,
    // a fetched page with its title and its site, and one with none, whose long address wraps and isn't repeated
    render: opened([visited(1, ACT, { title: RESULTS[0]!.title }), visited(2, DHC)]),
  },
  {
    id: "WEB-09",
    words: sourcesWords("{name} opened this page on OpenAI's computers, not on this one."),
    screen: SCREEN,
    render: opened(
      [visited(1, ACT, { title: RESULTS[0]!.title, via: "codex_open_page" })],
      "codex-acp",
    ),
  },
  {
    id: "WEB-10",
    words: sourcesWords("Opened '{page}' in the browser."),
    screen: SCREEN,
    render: opened([visited(1, DHC, { title: "Delhi High Court: judgments", via: "browser" })]),
  },
  {
    id: "WEB-11",
    words: sourcesWords("Read {page} online."),
    screen: SCREEN,
    // a Hindi query and a Hindi page title, each marked Hindi
    render: opened([
      searched(1, "दिल्ली किराया नियंत्रण अधिनियम बेदखली नोटिस", [
        { title: "दिल्ली किराया नियंत्रण अधिनियम, 1958 की धारा 14", url: ACT },
      ]),
      visited(2, ACT, { title: "दिल्ली किराया नियंत्रण अधिनियम, 1958 की धारा 14" }),
    ]),
  },
  {
    id: "WEB-12",
    words: sourcesWords("{n} sources"),
    screen: SCREEN,
    // shut, as an answer first shows it: five rows
    render: shut(MANY),
  },
  {
    id: "WEB-13",
    words: sourcesWords("1 source"),
    screen: SCREEN,
    render: opened([visited(1, ACT, { title: RESULTS[0]!.title })]),
  },
  {
    id: "WEB-14",
    words: sourcesWords("Opened {page} in the browser."),
    screen: SCREEN,
    // a page the browser opened with no title: its address, unquoted, with no site after it
    render: opened([visited(1, DHC, { via: "browser" })]),
  },
];
