// The copy review entries for the web sources at the end of an answer (room/thread/sources.tsx), apart from
// registry.tsx. Ids are WEB-01 on, since the registry's id rule (review/status.ts) takes a dash then digits. Each is the
// real room over FakeTransport: one question, the tool calls that searched and read, each carrying its `web`
// (ToolCallWeb) on its entry, the answer, and the step. WEB-12 and WEB-13 show the list folded, as an answer first
// does; every other entry shows it open from the start (SourcesOpenAtFirst), so its row is there without a click, and
// WEB-09 and WEB-10 show the words behind their icon, its tip open.
import { src, type SourcesKey } from "@clauseo/components/lib/words-sources.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import { pageOf, searchOf, webCall, webOf } from "../testing/web-sources.ts";
import type { AgentId, PageVia, ToolCallWeb, ViewEntry, WebResult } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { SourcesOpenAtFirst } from "../room/thread/sources.ts";
import { RoomClicking, RoomFocusing, RoomOver } from "./states.tsx";
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

/** A tool call the thread shows: its `call-<seq>` id and the web it carries. */
type Call = { seq: number; web: ToolCallWeb };
/** A web search the tool call at `seq` ran, with the results the agent got. */
const searched = (seq: number, query: string, results: WebResult[] = []): Call => ({
  seq,
  web: webOf({ search: searchOf(query, results) }),
});
/** A page the tool call at `seq` opened, `via` web_fetch by default, with its title when it has one. */
const visited = (seq: number, url: string, opts?: { title?: string; via?: PageVia }): Call => ({
  seq,
  web: webOf({ pages: [pageOf(url, opts)] }),
});

/** One tool call entry for each call, in order from 10, titled as the agent's adapter titles it. */
const callsOf = (calls: Call[], agent: AgentId): ViewEntry[] =>
  calls.map((c, i) => webCall(10 + i, `call-${c.seq}`, c.web, agent));

/** The thread's view: the question, a tool call for each one, carrying its web, the answer and its step. */
const entriesFor = (calls: Call[], agent: AgentId): ViewEntry[] => [
  QUESTION,
  ...callsOf(calls, agent),
  ANSWER,
  step(50),
];

const hostFor = (calls: Call[], agent: AgentId) => () =>
  threadHost({ entries: entriesFor(calls, agent), agent });

/** The list shut, as an answer first shows it. */
const shut =
  (calls: Call[], agent: AgentId = "claude-acp") =>
  () => <RoomOver host={hostFor(calls, agent)} />;
/** The list open from the start, as the review shows a row's words: the room's own default stays shut. */
const opened =
  (calls: Call[], agent: AgentId = "claude-acp") =>
  () => (
    <SourcesOpenAtFirst value>
      <RoomOver host={hostFor(calls, agent)} />
    </SourcesOpenAtFirst>
  );

/** The list open, then the control `selector` names pressed once it shows: for words behind an info tip. */
const pressing = (calls: Call[], agent: AgentId, selector: string) => () => (
  <SourcesOpenAtFirst value>
    <RoomClicking host={hostFor(calls, agent)} clicks={[selector]} />
  </SourcesOpenAtFirst>
);
/** The list open, then the element `selector` names focused from the keyboard: for words in an icon's tooltip. */
const focusing = (calls: Call[], agent: AgentId, selector: string) => () => (
  <SourcesOpenAtFirst value>
    <RoomFocusing host={hostFor(calls, agent)} focus={selector} />
  </SourcesOpenAtFirst>
);

const MANY: Call[] = [
  searched(1, "Delhi Rent Control Act notice period", RESULTS),
  searched(2, "section 14 eviction notice thirty days"),
  visited(3, ACT, { title: "Section 14 in the Delhi Rent Control Act, 1958" }),
  visited(4, DHC),
  visited(5, "https://www.livelaw.in/rent-notice", {
    title: "Notice to quit under the Rent Control Act",
  }),
];

// WEB-01 to WEB-04 named the old summary's two sentences ("2 web searches. 3 pages read."), which repeated the step
// chip's counts; they are retired, not reused. Round 3 (icons first) retired the rows' sentences: WEB-08 and WEB-15
// ("Read '{page}' online.", "Read {page} online.") and WEB-14 ("Opened {page} in the browser."), whose rows are now a
// tile, a link and a site, with no words; WEB-05, 06, 07, 09, 10 and 11 keep their ids for their lines' new words.
export const WEB_SOURCES: Entry[] = [
  {
    id: "WEB-05",
    words: sourcesWords("'{query}'"),
    screen: SCREEN,
    // the whole list open: a search with its results nested as tiles, links and sites, a search with none, a titled
    // page, a page with no title (its file name the link, its site after it) and another titled page
    render: opened(MANY),
    // tall enough for every row under the answer
    height: 900,
  },
  {
    id: "WEB-06",
    words: sourcesWords("No results"),
    screen: SCREEN,
    render: opened([searched(1, "rent control notice Karol Bagh 1987")]),
  },
  {
    id: "WEB-07",
    words: sourcesWords("{name} doesn't list its pages"),
    screen: SCREEN,
    render: opened([searched(1, "Delhi Rent Control Act notice period", [])], "codex-acp"),
  },
  {
    id: "WEB-09",
    words: sourcesWords("Opened on OpenAI's side"),
    screen: SCREEN,
    // Codex's open page, its info tip pressed open
    render: pressing(
      [visited(1, ACT, { title: RESULTS[0]!.title, via: "codex_open_page" })],
      "codex-acp",
      `button[aria-label="${src("en", "Opened on OpenAI's side")}"]`,
    ),
  },
  {
    id: "WEB-10",
    words: sourcesWords("Opened in the browser"),
    screen: SCREEN,
    // the browser glyph in the tile's place, its tooltip shown from the keyboard
    render: focusing(
      [visited(1, DHC, { title: "Delhi High Court: judgments", via: "browser" })],
      "claude-acp",
      '[data-testid="web-source-browser"]',
    ),
  },
  {
    id: "WEB-11",
    words: sourcesWords("'{query}'"),
    screen: SCREEN,
    // a Hindi query, a Hindi result and a Hindi page title, each marked Hindi
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
    // folded, as an answer first shows it: three distinct pages, the first two sites' tiles and the search glyph
    render: shut(MANY),
  },
  {
    id: "WEB-13",
    words: sourcesWords("1 source"),
    screen: SCREEN,
    // folded: one page, its one tile
    render: shut([visited(1, ACT, { title: RESULTS[0]!.title })]),
  },
  {
    id: "WEB-16",
    words: sourcesWords("Searches"),
    screen: SCREEN,
    // folded: a search that lists no page, so no count; the search glyph and the chevron, the button named Searches
    render: shut([searched(1, "Delhi Rent Control Act notice period", [])], "codex-acp"),
  },
];
