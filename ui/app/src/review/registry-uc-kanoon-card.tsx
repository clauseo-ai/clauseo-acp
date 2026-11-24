// The home card offering Clauseo Case Law (UC-1090 to UC-1096; 24 Nov round, issue 8): the real room over a
// scripted host, a matter with several papers open on a new conversation before its first question, and the
// connectors' state on the app feed with the case-law connector listed and not added, or (UC-1096) not listed, as when
// its liveness check failed. "Add Clauseo Case Law" opens Skills and connectors at Clauseo Case Law; its Add opens
// Settings, which this host answers as if the lawyer added it there. The price is the smallest pack's as given on 24
// Nov (₹236, GST included, 2,000 credits); the host sends the live one. These ids are registered once, here; the
// library's Case Law card (registry-uc-library.tsx) shares them.
import { cr, type ConnectorsRoomKey } from "@clauseo/components/lib/words-connectors-room.ts";
import { detailed, fakeNext } from "../room/fake-next.ts";
import type { PaperSummary } from "../room/host-next.ts";
import { uuid } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { ConnectorStatus, ConnectorsState } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import type { WordsRef } from "./words.ts";

const roomWords = (key: ConnectorsRoomKey): WordsRef => ({
  file: "lib/words-connectors-room.ts",
  key,
  text: (lang) => cr(lang, key),
  later: cr.hi[key] === null,
});

const PAPERS: PaperSummary[] = [
  "Plaint.pdf",
  "Written statement.pdf",
  "Lease deed 2019.pdf",
  "Notice under section 106.pdf",
  "Rent receipts.xlsx",
  "Rejoinder draft.docx",
].map((path, i) => ({ file: uuid(i + 1, 8), path, size: 120_000, modified: 1_730_000_000_000 }));

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

/** The matter open on a new conversation, Case Law not added (listed, or not when `listed` is false); Settings' add
 * answered as the host would. */
export function kanoonHost(listed = true): FakeTransport {
  const transport: FakeTransport = new FakeTransport({
    next: {
      ...fakeNext({ matters: () => [detailed(MATTER, { last_opened: 1 })], papers: PAPERS }),
      first: () => [
        {
          connectors_changed: listed
            ? stateOf("not_added")
            : { available: true, locked: false, connectors: [] },
        },
      ],
    },
    openSettings: () =>
      transport.appEvent({
        connectors_changed: stateOf({ signed_in: { low: false, balance: { credits: 200 } } }),
      }),
  });
  return transport;
}

const render = () => <RoomOver host={() => kanoonHost()} />;

const entry = (id: string, key: ConnectorsRoomKey): Entry => ({
  id,
  words: roomWords(key),
  screen: "room/case-law-offer",
  render,
});

export const UC_KANOON_CARD: Entry[] = [
  entry(
    "UC-1090",
    "Find and read Supreme Court, High Court and tribunal judgments and central Acts, and see who cites each judgment.",
  ),
  entry("UC-1091", "Add Clauseo Case Law"),
  entry("UC-1092", "Clauseo Case Law"),
  entry("UC-1093", "From {price}, about {n} searches"),
  entry("UC-1095", "Prepaid, including GST. No subscription."),
  entry("UC-1094", "Powered by Indian Kanoon"),
  {
    id: "UC-1096",
    words: roomWords("Prepaid credits. No subscription."),
    screen: "room/case-law-offer",
    render: () => <RoomOver host={() => kanoonHost(false)} />,
  },
];
