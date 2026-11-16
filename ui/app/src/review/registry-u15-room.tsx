// U15's copy review entries for the room (the main window): the case-law hint under the answer of the turn the host
// named, with the "from" price and without it, and the out-of-credit note under the composer's field, each in the real
// room over FakeTransport, the connectors' state and the hint arriving on the app feed as the host sends them. Hidden
// in a conversation, the note shows nothing, so that state has no entry; the tests cover it
// (room/connectors/connectors.test.tsx). Figures are review fixtures, not prices.
import { cr, type ConnectorsRoomKey } from "@clauseo/components/lib/words-connectors-room.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { ConnectorStatus, ConnectorsState } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import type { WordsRef } from "./words.ts";

/** A string of the room's connector parts (words-connectors-room.ts). */
const roomWords = (key: ConnectorsRoomKey): WordsRef => ({
  file: "lib/words-connectors-room.ts",
  key,
  text: (lang) => cr(lang, key),
  later: cr.hi[key] === null,
});

const connectors = (status: ConnectorStatus, price: number | null): ConnectorsState => ({
  available: true,
  locked: false,
  connectors: [
    {
      id: "case-law",
      name: "Indian Kanoon",
      // the smallest pack's gross price, GST included: 200 credits for ₹23.60
      ...(price === null ? {} : { price_paise: price }),
      packs: [{ pack: "p200", credits: 200, paise: 2_360 }],
      status,
    },
  ],
});

/** The open conversation, its answer to a case-law question ended. */
const ENTRIES = [
  userMessage(5, 1, "Find Supreme Court judgments on section 138 of the NI Act."),
  agentText(
    6,
    1,
    "The web search found three Supreme Court judgments on dishonoured cheques under section 138.",
  ),
  step(9),
];

const STATES = {
  hint: { entries: ENTRIES, connectors: connectors("not_added", 2_360), hint: true },
  "hint-no-price": { entries: ENTRIES, connectors: connectors("not_added", null), hint: true },
  "out-of-credit": { entries: ENTRIES, connectors: connectors({ out_of_credit: {} }, 2_360) },
} satisfies Record<string, ThreadState>;

const entry = (
  id: string,
  key: ConnectorsRoomKey,
  row: keyof typeof STATES,
  screen: string,
): Entry => ({
  id,
  words: roomWords(key),
  screen,
  render: () => <RoomOver host={() => threadHost(STATES[row])} />,
});

/** The room's entries, each id written as it is. */
export const U15_ROOM: Entry[] = [
  entry(
    "U15-58",
    "For judgments, Clauseo can search Indian Kanoon directly.",
    "hint",
    "room/case-law-hint",
  ),
  entry("U15-59", "It's a paid add-on, from {price}, including GST.", "hint", "room/case-law-hint"),
  entry("U15-60", "It's a paid add-on.", "hint-no-price", "room/case-law-hint"),
  entry("U15-61", "Set up in Settings", "hint", "room/case-law-hint"),
  entry("U15-62", "Don't show again", "hint", "room/case-law-hint"),
  entry("U15-63", "Indian Kanoon suggestion", "hint", "room/case-law-hint"),
  entry("U15-64", "Out of case-law credits.", "out-of-credit", "room/out-of-credit"),
  entry(
    "U15-65",
    "Searches of Indian Kanoon stop until you add more.",
    "out-of-credit",
    "room/out-of-credit",
  ),
  entry("U15-66", "Add credits", "out-of-credit", "room/out-of-credit"),
  entry("U15-75", "Hide in this conversation", "out-of-credit", "room/out-of-credit"),
];
