// Clauseo Case Law's credit said the same everywhere (r7363, r7381, r7458, r7462; UC-1249, UC-1291, UC-1460, UC-1462 to
// UC-1464): the real room over a scripted host by the composer, and Settings › Connectors. Out of credit (UC-1249's
// line), a balance of 0 the host still calls signed in (UC-1291), a conversation holding the host's
// case_law_out_of_credit notice after its last message once credit is back (UC-1460 by the composer, UC-1462 the
// notice's row), and Settings out of credit with no packs and no pay link (UC-1463, UC-1464). The library shows the
// same words; the tests cover it (room/connectors/credits-everywhere.test.tsx).
import { cr, type ConnectorsRoomKey } from "@clauseo/components/lib/words-connectors-room.ts";
import { CASE_LAW_OUT_OF_CREDIT } from "../../room/thread/case-law-notice.ts";
import { agentText, step, toolCall, userMessage, uuid } from "../../testing/stream-script.ts";
import type { ConnectorStatus, ConnectorsState, ViewEntry } from "../../transport/protocol.ts";
import type { Entry } from "../entry.ts";
import { threadHost } from "../hosts.ts";
import { RoomOver } from "../states.tsx";
import { SettingsOver } from "../states-settings.tsx";
import type { WordsRef } from "../words.ts";

const roomWords = (key: ConnectorsRoomKey): WordsRef => ({
  file: "lib/words-connectors-room.ts",
  key,
  text: (lang) => cr(lang, key),
  later: cr.hi[key] === null,
});

const stateOf = (status: ConnectorStatus, packs = true): ConnectorsState => ({
  available: true,
  locked: false,
  connectors: [
    {
      id: "case-law",
      name: "Clauseo Case Law",
      price_paise: 23_600,
      packs: packs ? [{ pack: "p2000", credits: 2_000, paise: 23_600 }] : [],
      status,
    },
  ],
});

const QUESTION = userMessage(5, 1, "Find Supreme Court judgments on section 138 of the NI Act.");
/** The host's mark after the refused step (its kind's final name; the binding is on its way). */
const MARK: ViewEntry = {
  notice: {
    order: 7,
    event: uuid(7, 10),
    level: "warning",
    text: "Clauseo Case Law is out of credit.",
    kind: CASE_LAW_OUT_OF_CREDIT as never,
    ts: 7_000,
  },
};
const REFUSED = [
  QUESTION,
  toolCall(6, 1, "failed"),
  MARK,
  agentText(8, 1, "The search was refused."),
  step(9),
];

const room = (entries: ViewEntry[], status: ConnectorStatus) => () => (
  <RoomOver host={() => threadHost({ entries, connectors: stateOf(status) })} />
);
const settings =
  (status: ConnectorStatus) => (lang: Parameters<NonNullable<Entry["render"]>>[0]) => (
    <SettingsOver section="connectors" lang={lang} connectors={stateOf(status, false)} />
  );
const PAID: ConnectorStatus = { signed_in: { low: false, balance: { credits: 2_000 } } };
const OUT: ConnectorStatus = { out_of_credit: {} };
const ZERO: ConnectorStatus = { signed_in: { low: true, balance: { credits: 0 } } };

const entry = (
  id: string,
  key: ConnectorsRoomKey,
  render: Entry["render"],
  screen: string,
): Entry => ({
  id,
  words: roomWords(key),
  screen,
  render,
});

const UC_CREDITS: Entry[] = [
  entry(
    "UC-1249",
    "Not enough credits for a search. Add credits to keep searching.",
    room(REFUSED, OUT),
    "room/out-of-credit",
  ),
  entry("UC-1291", "No credits. Add credits to search.", room(REFUSED, ZERO), "room/out-of-credit"),
  entry(
    "UC-1460",
    "Credits added. Ask again to search case law.",
    room(REFUSED, PAID),
    "room/out-of-credit",
  ),
  entry(
    "UC-1462",
    "Clauseo Case Law had no credits for this search.",
    room(REFUSED, PAID),
    "room/thread",
  ),
  entry("UC-1463", "Not enough credits for a search.", settings(OUT), "settings/connectors"),
  entry("UC-1464", "No credits.", settings(ZERO), "settings/connectors"),
];

export default UC_CREDITS;
