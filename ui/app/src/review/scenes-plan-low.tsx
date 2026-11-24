// Looks for the 24 Nov plan-low change (scenes.tsx opens them by id; no strings of their own): the composer's warning
// before sending at 78% of the 5-hour window and 92% of the weekly one, the plan's meter amber at 80% and red at 96%
// with its card open, Settings' Plan and usage with both colours, and the case-law running-low line by the composer.
// The real room over FakeTransport, on a matter with four papers and a long question.
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import type { ConnectorsState, PaperSummary, ViewEntry } from "../transport/protocol.ts";
import { agentText, step, userMessage, uuid } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { figures, weekly } from "./registry-u12.tsx";
import { RoomClicking, RoomOver } from "./states.tsx";
import { SettingsOver } from "./states-settings.tsx";
import { limitThreadWords, limitWords, type WordsRef } from "./words.ts";

const PAPERS: PaperSummary[] = [
  "Lease deed.pdf",
  "Notice from the bank.pdf",
  "Reply to notice (draft).docx",
  "Loan agreement.pdf",
].map((path, i) => ({ file: uuid(i + 1, 8), path, size: 1000, modified: 1_730_000_000_000 }));

const ASKED =
  "Read the lease deed, the loan agreement and the bank's notice, and tell me whether the notice was served in " +
  "time under clause nine, what the bank must show to recover the sum, and draft a reply that asks it to withdraw.";
const ENTRIES: ViewEntry[] = [
  userMessage(5, 1, ASKED),
  agentText(
    6,
    1,
    "The notice was served on 2 October, twelve days after the thirty days clause nine allows ran out, so it is " +
      "late. To recover the sum the bank must show a valid notice and the default it names. The draft reply is " +
      "in Reply to notice (draft).docx.",
  ),
  step(9),
];

const room = (over: Partial<ThreadState>): ThreadState => ({
  entries: ENTRIES,
  papers: PAPERS,
  ...over,
});
const five = figures(30).windows[0]!;

const LOW: ConnectorsState = {
  available: true,
  locked: false,
  connectors: [
    {
      id: "case-law",
      name: "Indian Kanoon",
      price_paise: 2_360,
      packs: [{ pack: "p200", credits: 200, paise: 2_360 }],
      status: { signed_in: { low: true, balance: { credits: 8 } } },
    },
  ],
};
const LOW_WORDS: WordsRef = {
  file: "lib/words-connectors.ts",
  key: "Running low. Add credits to keep searching.",
  text: (lang) => cw(lang, "Running low. Add credits to keep searching."),
  later: false,
};

export const PLAN_LOW_SCENES: readonly Entry[] = [
  {
    id: "plan-low/composer-78",
    words: limitThreadWords("You've used {n}% of your 5-hour {name} limit."),
    screen: "limits/warning",
    render: () => <RoomOver host={() => threadHost(room({ limits: { claude: figures(78) } }))} />,
  },
  {
    id: "plan-low/composer-weekly-92",
    words: limitThreadWords("You've used {n}% of your weekly {name} limit."),
    screen: "limits/warning",
    render: () => (
      <RoomOver
        host={() =>
          threadHost(room({ limits: { claude: figures(30, { windows: [five, weekly(92)] }) } }))
        }
      />
    ),
  },
  ...(
    [
      ["plan-low/meter-amber", 80],
      ["plan-low/meter-red", 96],
    ] as const
  ).map(([id, used]): Entry => ({
    id,
    words: limitWords("Close to the limit. Resets at {time}."),
    screen: "limits/plan",
    render: () => (
      <RoomClicking
        host={() => threadHost(room({ limits: { claude: figures(used) } }))}
        clicks={['[data-testid="plan"]']}
      />
    ),
  })),
  {
    id: "plan-low/settings",
    words: limitWords("Close to the limit. Resets at {time}."),
    screen: "settings/usage",
    width: 960,
    height: 700,
    render: (lang) => (
      <SettingsOver
        section="usage"
        lang={lang}
        limits={{
          claude: figures(80, { windows: [figures(80).windows[0]!, weekly(96)] }),
          codex: figures(40, { plan: "ChatGPT Plus" }),
        }}
      />
    ),
  },
  {
    id: "plan-low/case-law-low",
    words: LOW_WORDS,
    screen: "room/low-credit",
    render: () => (
      <RoomOver
        host={() => threadHost(room({ limits: { claude: figures(30) }, connectors: LOW }))}
      />
    ),
  },
];
