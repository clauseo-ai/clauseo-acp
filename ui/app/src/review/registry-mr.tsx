// The read invite's copy review entries (UC-351 on), apart from registry.tsx to keep that file under the size limit:
// letting a conversation read another matter's files. UC-351, the open conversation's menu item with its submenu of the
// other matters, in the real rail (triage-states.tsx's room: Sharma v. State of Delhi open); UC-352 to UC-354, the pills
// over the composer for a conversation that may read Joshi's and Kapoor's matters. The refusal's line and its offer
// are the notices' (registry-notices.tsx, UC-346 to UC-350).
import type { MatterSummary } from "../transport/protocol.ts";
import { uuid, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { RoomTriage } from "./triage-states.tsx";
import { threadWords, triageLateWords } from "./words.ts";

const JOSHI: MatterSummary = { matter: uuid(2, 6), name: "Joshi v. Joshi", kind: "folder" };
const KAPOOR: MatterSummary = {
  matter: uuid(3, 6),
  name: "Kapoor Exports v. State Bank of India",
  kind: "folder",
};

/** A conversation that may read Joshi's and Kapoor's matters: its pills over the composer. */
const reading = () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [userMessage(5, 1, "Compare the rent clause with the one in Joshi's lease.")],
        others: [JOSHI, KAPOOR],
        read: [JOSHI.matter, KAPOOR.matter],
      })
    }
  />
);

export const MR: Entry[] = [
  {
    id: "UC-351",
    words: triageLateWords("Let this conversation read"),
    screen: "matters/rail-triage",
    width: 1280,
    height: 1100,
    render: () => (
      <RoomTriage
        acts={[
          { press: '[aria-label="Actions for Bail conditions for Mr. Sharma"]' },
          { item: "Let this conversation read" },
        ]}
      />
    ),
  },
  {
    id: "UC-352",
    words: threadWords("Can read {matter}'s files"),
    screen: "room/composer",
    render: reading,
  },
  {
    // the pill's ×, named for assistive technology
    id: "UC-353",
    words: threadWords("Stop letting this conversation read {matter}'s files"),
    screen: "room/composer",
    render: reading,
  },
  {
    // the pills' list, named for assistive technology
    id: "UC-354",
    words: threadWords("Other matters this conversation can read"),
    screen: "room/composer",
    render: reading,
  },
];
