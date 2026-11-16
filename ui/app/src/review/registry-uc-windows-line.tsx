// UC-820: what the Safe workspace can't stop on Windows, then UC-850's Mac advice (Rohan's call), in the real room over a Windows host listing two matters,
// before "Got it" (room/windows-line/). Settings' Safe workspace on Windows says the same words, always while it's on.
// Provisional, hi: later. UC-821, the Mac's counterpart, comes after host PRs #1085 and #1153.
import { WINDOWS_LINE, WINDOWS_STRICT } from "@clauseo/components/lib/words-protection.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { MatterSummary } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { protectionWords } from "./words.ts";

const SEEN = "clauseo.windows-line-seen";

const SECOND: MatterSummary = {
  matter: "00000000-0000-4000-8000-0000000000a2",
  name: "Sharma v. Sharma",
  kind: "folder",
};

/** A Windows host listing two matters, with the notice not yet put away on this page (the frames share one
 * localStorage). */
const twoMatters = (): FakeTransport => {
  localStorage.removeItem(SEEN);
  return new FakeTransport({
    platform: "windows",
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER, SECOND] };
      throw new Error(`the review's host answers list_matters only, not ${JSON.stringify(query)}`);
    },
  });
};

export const UC_WINDOWS_LINE: Entry[] = [
  {
    id: "UC-820",
    words: protectionWords(WINDOWS_LINE),
    screen: "room/windows-line",
    render: () => <RoomOver host={twoMatters} />,
  },
  {
    id: "UC-850",
    words: protectionWords(WINDOWS_STRICT),
    screen: "room/windows-line",
    render: () => <RoomOver host={twoMatters} />,
  },
];
