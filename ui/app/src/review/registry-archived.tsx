// The copy review's entry for an archived conversation (archived.tsx), apart from registry.tsx to keep that file under
// the size limit: the real room over FakeTransport, Find typed into and its archived result pressed, as the lawyer
// would (RoomAfter's steps). The conversation's entries show, and its line stands in the composer's place.
import { agentText, batch, event, reset, userMessage } from "../testing/stream-script.ts";
import type { MatterSummary, SearchHit, ViewEntry } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { fakeNext, railStates } from "../room/fake-next.ts";
import type { ThreadInfo } from "../room/host-next.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomFound } from "./states.tsx";
import { threadWords } from "./words.ts";

const SCRATCH: MatterSummary = {
  matter: "00000000-0000-4000-8000-000000000099",
  name: "Scratch",
  kind: "scratch",
};
const BAIL = "00000000-0000-7000-8000-0000000000b1";
const OLD = "00000000-0000-7000-8000-0000000000b2";

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

/** Find's one result: a question in the archived conversation. */
const HIT: SearchHit = {
  kind: "thread_title",
  matter: MATTER.matter,
  thread: OLD,
  thread_title: "Surety in the earlier bail order",
  at: Date.now(),
  snippet: [{ text: "Surety", hit: true }],
};

const headerOf = (thread: string, status: "idle" | "archived"): ViewEntry => ({
  header: {
    order: 1,
    thread,
    matter: MATTER.matter,
    agent: "claude-acp",
    status,
    config_options: [],
    commands: [],
    plan: [],
  },
});

/** Each thread's window: the archived one's two exchanges, the live one's one. */
const windowOf = (stream: string) =>
  stream === `thread:${OLD}`
    ? reset(
        [
          userMessage(2, 1, "What surety did the earlier bail order ask for?"),
          agentText(
            3,
            1,
            "The order of 12 August asked for a personal bond of ₹25,000 with one surety of the same amount, a resident of the district.",
          ),
          userMessage(4, 2, "Did the accused furnish it?"),
          agentText(
            5,
            2,
            "Yes. The surety bond was filed on 16 August and accepted the same day, as the order sheet records.",
          ),
        ],
        headerOf(OLD, "archived"),
      )
    : reset([userMessage(2, 3, "What conditions can we offer for bail?")], headerOf(BAIL, "idle"));

const archivedHost = (): FakeTransport => {
  const transport: FakeTransport = new FakeTransport({
    next: fakeNext({
      matters: () => railStates(MATTER, SCRATCH, Date.now()),
      threads: (m) => (m === MATTER.matter ? threads() : []),
    }),
    query: (q) => {
      if (typeof q === "object" && "search" in q)
        return { search: { page: { hits: [HIT], mode: "ranked" } } };
      throw new Error("not scripted");
    },
    subscribe: (stream) =>
      queueMicrotask(() =>
        transport.deliver(event(batch(1, [windowOf(stream)], undefined, stream))),
      ),
  });
  return transport;
};

export const ARCHIVED: readonly Entry[] = [
  {
    id: "AR-01",
    words: threadWords("Archived. Read only."),
    screen: "room/archived",
    width: 1280,
    height: 800,
    render: () => <RoomFound host={archivedHost} words="surety" />,
  },
];
