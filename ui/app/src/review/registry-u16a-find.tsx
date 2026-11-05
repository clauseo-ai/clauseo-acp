// U16a's copy review entries for Find searching inside (RO-20 to RO-28 but RO-23, the rail's own status), apart from
// registry.tsx to keep that file under the size limit: each in the real rail over FakeTransport, the host answering
// `search` with matters, a paper, conversations found by title and by what was said, one of them archived; while the
// index builds; and with nothing found. Find is typed into as the lawyer would (RoomAfter's `find` step).
import type { MatterSummary, SearchHit, SearchPage } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { fakeNext, railStates } from "../room/fake-next.ts";
import type { ThreadInfo } from "../room/host-next.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomAfter, RoomOver } from "./states.tsx";
import { findWords, matterWords } from "./words.ts";

const RAIL = { width: 1280, height: 900 } as const;

const SCRATCH: MatterSummary = {
  matter: "00000000-0000-4000-8000-000000000099",
  name: "Scratch",
  kind: "scratch",
};
const BAIL = "00000000-0000-7000-8000-0000000000b1";
const OLD = "00000000-0000-7000-8000-0000000000b2";
/** Kapoor Exports v. State Bank of India, in railStates. */
const KAPOOR = "01900002-0000-7000-8000-000000000002";

const day = (days: number, h = 10) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days, h, 0).getTime();
};

const threads = (): ThreadInfo[] => [
  {
    thread: BAIL,
    matter: MATTER.matter,
    agent: "claude-acp",
    status: "idle",
    seen: true,
    title: "Bail conditions for the accused",
  },
  {
    thread: OLD,
    matter: MATTER.matter,
    agent: "claude-acp",
    status: "archived",
    seen: true,
    title: "Surety in the earlier bail order",
  },
];

const HITS: SearchHit[] = [
  {
    kind: "file_name",
    matter: MATTER.matter,
    path: "Bail/Surety bond, 14 October.pdf",
    at: day(-2),
    snippet: [
      { text: "Surety", hit: true },
      { text: " bond, 14 October.pdf", hit: false },
    ],
  },
  {
    kind: "thread_title",
    matter: MATTER.matter,
    thread: OLD,
    thread_title: "Surety in the earlier bail order",
    at: day(-9),
    snippet: [{ text: "Surety", hit: true }],
  },
  {
    kind: "answer",
    matter: MATTER.matter,
    thread: BAIL,
    thread_title: "Bail conditions for the accused",
    order: 6,
    at: day(0),
    snippet: [
      { text: "A personal bond of ₹50,000 with one ", hit: false },
      { text: "surety", hit: true },
      { text: ".", hit: false },
    ],
  },
  {
    kind: "prompt",
    matter: KAPOOR,
    thread: "00000000-0000-7000-8000-0000000000b3",
    thread_title: "Counter to the stay application",
    order: 2,
    at: day(-1),
    snippet: [
      { text: "Can we offer a ", hit: false },
      { text: "surety", hit: true },
      { text: " instead of a cash deposit?", hit: false },
    ],
  },
];

/** The rail in every state, its open matter's conversations, and a host answering search with `hits` (and `over`). */
const findHost =
  (hits: SearchHit[], over: Partial<SearchPage> = {}) =>
  (): FakeTransport =>
    new FakeTransport({
      next: fakeNext({
        matters: () => railStates(MATTER, SCRATCH, Date.now()),
        threads: (m) => (m === MATTER.matter ? threads() : []),
      }),
      query: (q) => {
        if (typeof q === "object" && "search" in q)
          return { search: { page: { hits, mode: "ranked", ...over } } };
        throw new Error("not scripted");
      },
    });

const found =
  (hits = HITS, over: Partial<SearchPage> = {}, words = "surety") =>
  () => <RoomAfter host={findHost(hits, over)} steps={[{ find: words }]} />;

export const U16A_FIND: readonly Entry[] = [
  {
    id: "RO-20",
    words: matterWords("Find"),
    screen: "matters/rail-find",
    width: RAIL.width,
    height: 300,
    render: () => <RoomOver host={findHost(HITS)} />,
  },
  {
    id: "RO-21",
    words: findWords("Conversations"),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(),
  },
  {
    id: "RO-29",
    words: findWords("{matter}, {day}"),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(),
  },
  {
    id: "RO-30",
    words: findWords("today"),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(),
  },
  {
    id: "RO-22",
    words: findWords("Searches titles, paper names and conversations."),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(),
  },
  {
    id: "RO-24",
    words: findWords("You:"),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(),
  },
  {
    id: "RO-31",
    words: findWords("Clauseo:"),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(),
  },
  {
    id: "RO-25",
    words: findWords("Paper, {file}"),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(),
  },
  {
    id: "RO-26",
    words: findWords('Nothing found for "{words}".'),
    screen: "matters/rail-find",
    ...RAIL,
    render: found([], {}, "Zaveri"),
  },
  {
    id: "RO-27",
    words: findWords("Still adding older conversations."),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(HITS, { indexing: { done: 120, total: 480 } }),
  },
  {
    id: "RO-32",
    words: findWords("Some may not show yet."),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(HITS, { indexing: { done: 120, total: 480 } }),
  },
  {
    id: "RO-28",
    words: findWords("Archived"),
    screen: "matters/rail-find",
    ...RAIL,
    render: found(),
  },
];
