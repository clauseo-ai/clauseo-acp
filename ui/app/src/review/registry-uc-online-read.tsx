// UC's entries for the wall's dimmed tiles, reworded for a paper only online that reading it downloads (UC-913 to
// UC-915, copy-approvals/UC-online-only-read.json): the new conversation's clause for one or several papers only in a
// named sync service (UC-913, UC-914), and a file only online's tip (UC-915). They show the states registry-wall-away.tsx
// renders, in the real room over a scripted host.
import type { Entry } from "./entry.ts";
import { STATES } from "./registry-wall-away.tsx";
import { matterWords, roomWords } from "./words.ts";

export const UC_ONLINE_READ: Entry[] = [
  {
    id: "UC-913",
    words: matterWords("1 more is only in {service}. Reading it downloads it to this computer."),
    screen: "room/wall-away",
    render: STATES.mac,
  },
  {
    id: "UC-914",
    words: matterWords(
      "{n} more are only in {service}. Reading them downloads them to this computer.",
    ),
    screen: "room/wall-away",
    render: STATES.windows,
  },
  {
    id: "UC-915",
    words: roomWords(
      "Not on this computer, so Clauseo saves no new versions of it for now. Reading it in a conversation downloads it.",
    ),
    screen: "room/wall-away",
    render: STATES.other,
  },
];
