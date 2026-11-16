// UC-820: what Clauseo can't keep apart on Windows, then UC-850's Mac advice (Rohan's call), in the real room over a
// Windows host listing two matters, before "Got it" (room/windows-line/). Settings › Privacy on Windows says the same
// words for good. Provisional, hi: later. UC-821, the Mac's counterpart, comes after host PRs #1085 and #1153.
import {
  wl,
  WINDOWS_LINE,
  WINDOWS_STRICT,
  type WindowsLineKey,
} from "@clauseo/components/lib/words-windows-line.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { MatterSummary } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import type { WordsRef } from "./words.ts";

/** A string of the Windows line's words (words-windows-line.ts). */
const words = (key: WindowsLineKey): WordsRef => ({
  file: "lib/words-windows-line.ts",
  key,
  text: (lang) => wl(lang, key),
  later: wl.hi[key] === null,
});

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
      // no conversations yet, so the room opens on the matter's empty work column
      if (typeof query === "object" && "list_threads" in query) return { threads: [] };
      throw new Error(
        `the review's host answers list_matters and list_threads only, not ${JSON.stringify(query)}`,
      );
    },
  });
};

export const UC_WINDOWS_LINE: Entry[] = [
  {
    id: "UC-820",
    words: words(WINDOWS_LINE),
    screen: "room/windows-line",
    render: () => <RoomOver host={twoMatters} />,
  },
  {
    id: "UC-850",
    words: words(WINDOWS_STRICT),
    screen: "room/windows-line",
    render: () => <RoomOver host={twoMatters} />,
  },
];
