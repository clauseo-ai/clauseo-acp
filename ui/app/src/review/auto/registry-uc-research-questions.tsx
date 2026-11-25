// What Clauseo Case Law's credits buy, in the server's own words (orchestrator r7470; UC-1466 to UC-1468): shown as
// written when the host passes the text on (#1330), nothing when it doesn't. The home card's price line with the
// smallest pack's words (UC-1466; the library's not-yet-added card says the same), the balance's words under its
// credits left in Settings › Connectors (UC-1467; the library's added card too), and each pack's line in Add credits
// (UC-1468). The texts are review fixtures, not the server's.
import { cr, type ConnectorsRoomKey } from "@clauseo/components/lib/words-connectors-room.ts";
import { cw, type ConnectorsKey } from "@clauseo/components/lib/words-connectors.ts";
import { detailed, fakeNext } from "../../room/fake-next.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type {
  Balance,
  ConnectorStatus,
  ConnectorsState,
  PackView,
} from "../../transport/protocol.ts";
import type { Entry } from "../entry.ts";
import { MATTER } from "../hosts.ts";
import { PressWords } from "../press-words.tsx";
import { RoomOver } from "../states.tsx";
import { SettingsOver } from "../states-settings.tsx";
import type { WordsRef } from "../words.ts";

const roomWords = (key: ConnectorsRoomKey): WordsRef => ({
  file: "lib/words-connectors-room.ts",
  key,
  text: (lang) => cr(lang, key),
  later: cr.hi[key] === null,
});

const connectorsWords = (key: ConnectorsKey): WordsRef => ({
  file: "lib/words-connectors.ts",
  key,
  text: (lang) => cw(lang, key),
  later: cw.hi[key] === null,
});

/** A pack or a balance with the server's words, as #1330's host passes them on. */
const saying = <T extends PackView | Balance>(value: T, text: string): T => ({
  ...value,
  research_questions_text: text,
});

const PACKS: PackView[] = [
  saying({ pack: "p2000", credits: 2_000, paise: 23_600 }, "about 5 research questions"),
  saying({ pack: "p10000", credits: 10_000, paise: 1_18_000 }, "about 25 research questions"),
];

const stateOf = (status: ConnectorStatus): ConnectorsState => ({
  available: true,
  locked: false,
  connectors: [
    { id: "case-law", name: "Clauseo Case Law", price_paise: 23_600, packs: PACKS, status },
  ],
});

const PAID: ConnectorStatus = {
  signed_in: {
    low: false,
    balance: saying({ credits: 1_240 }, "about 3 research questions"),
  },
};

/** A matter open on a new conversation, Case Law not added: the home card. */
const home = () => (
  <RoomOver
    host={() =>
      new FakeTransport({
        next: {
          ...fakeNext({ matters: () => [detailed(MATTER, { last_opened: 1 })] }),
          first: () => [{ connectors_changed: stateOf("not_added") }],
        },
      })
    }
  />
);

const UC_RESEARCH_QUESTIONS: Entry[] = [
  {
    id: "UC-1466",
    words: roomWords("From {price}, for {questions}."),
    screen: "room/case-law-offer",
    render: home,
  },
  {
    id: "UC-1467",
    words: roomWords("Enough for {questions}."),
    screen: "settings/connectors",
    width: 960,
    height: 700,
    render: (lang) => <SettingsOver section="connectors" lang={lang} connectors={stateOf(PAID)} />,
  },
  {
    id: "UC-1468",
    words: connectorsWords("{credits} credits, {rupees}, for {questions}"),
    screen: "settings/connectors",
    width: 960,
    height: 700,
    render: (lang) => (
      <PressWords words={cw(lang, "Add credits")}>
        <SettingsOver section="connectors" lang={lang} connectors={stateOf(PAID)} />
      </PressWords>
    ),
  },
];

export default UC_RESEARCH_QUESTIONS;
