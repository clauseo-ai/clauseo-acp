// Clauseo Case Law's credit said the same everywhere (r7363, r7381; UC-1249, UC-1291, UC-1460): the real room over a
// scripted host, by the composer. Out of credit with no figure read (UC-1249's line), a balance of 0 the host still
// calls signed in (UC-1291), and credits arriving after a turn ran as the credit went out (UC-1460): the host sends
// out_of_credit while the turn runs, then a balance. Settings and the library show the same words; the tests cover
// them (settings/sections/connectors-out.test.tsx, room/connectors/credits-everywhere.test.tsx).
import { cr, type ConnectorsRoomKey } from "@clauseo/components/lib/words-connectors-room.ts";
import { agentText, step, userMessage } from "../../testing/stream-script.ts";
import type { ConnectorStatus, ConnectorsState } from "../../transport/protocol.ts";
import type { Entry } from "../entry.ts";
import { threadHost } from "../hosts.ts";
import { RoomOver } from "../states.tsx";
import type { WordsRef } from "../words.ts";

const roomWords = (key: ConnectorsRoomKey): WordsRef => ({
  file: "lib/words-connectors-room.ts",
  key,
  text: (lang) => cr(lang, key),
  later: cr.hi[key] === null,
});

const stateOf = (status: ConnectorStatus): ConnectorsState => ({
  available: true,
  locked: false,
  connectors: [
    {
      id: "case-law",
      name: "Clauseo Case Law",
      price_paise: 23_600,
      packs: [{ pack: "p2000", credits: 2_000, paise: 23_600 }],
      status,
    },
  ],
});

const QUESTION = userMessage(5, 1, "Find Supreme Court judgments on section 138 of the NI Act.");
const ENDED = [QUESTION, agentText(6, 1, "The search was refused: no case-law credits."), step(9)];

const out = () => (
  <RoomOver
    host={() => threadHost({ entries: ENDED, connectors: stateOf({ out_of_credit: {} }) })}
  />
);
const zero = () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: ENDED,
        connectors: stateOf({ signed_in: { low: true, balance: { credits: 0 } } }),
      })
    }
  />
);
/** The turn runs as the credit goes out, then a paid recharge's balance arrives. */
const added = () => (
  <RoomOver
    host={() => {
      const transport = threadHost({
        entries: [QUESTION],
        status: "running",
        connectors: stateOf({ signed_in: { low: false, balance: { credits: 400 } } }),
      });
      setTimeout(
        () => transport.appEvent({ connectors_changed: stateOf({ out_of_credit: {} }) }),
        400,
      );
      setTimeout(
        () =>
          transport.appEvent({
            connectors_changed: stateOf({ signed_in: { low: false, balance: { credits: 2_000 } } }),
          }),
        800,
      );
      return transport;
    }}
  />
);

const UC_CREDITS: Entry[] = [
  {
    id: "UC-1249",
    words: roomWords("Not enough credits for a search. Add credits to keep searching."),
    screen: "room/out-of-credit",
    render: out,
  },
  {
    id: "UC-1291",
    words: roomWords("No credits. Add credits to search."),
    screen: "room/out-of-credit",
    render: zero,
  },
  {
    id: "UC-1460",
    words: roomWords("Credits added. Ask again to search case law."),
    screen: "room/out-of-credit",
    render: added,
  },
];

export default UC_CREDITS;
