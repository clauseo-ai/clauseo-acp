// The wall following the open matter's folder (decision record 0055), in the real room over FakeTransport: the matter
// open with one paper; a paper added in Finder a moment after the room shows, the host's papers_changed bringing its
// tile and the new conversation's count; and the folder deleted while the matter is open, the moved folder's line with
// Locate folder taking the thread's place and the wall emptying. The host's events arrive on the app feed a second
// after the room mounts, so the frame first shows the room as it was. Words reused from U6 and U2.
import type { Entry } from "./entry.ts";
import { LiveRoom } from "./papers-live-room.tsx";
import { matterWords } from "./words.ts";

/** The wall following the folder, each id written as it is. */
export const PAPERS_LIVE: Entry[] = [
  {
    id: "PL-01",
    words: matterWords("A new conversation. It reads this matter's one paper."),
    screen: "room/papers-live",
    render: () => <LiveRoom change="none" />,
  },
  {
    id: "PL-02",
    words: matterWords("A new conversation. It reads this matter's {n} papers."),
    screen: "room/papers-live",
    render: () => <LiveRoom change="added" />,
  },
  {
    id: "PL-03",
    words: matterWords("This matter's folder has moved."),
    screen: "room/papers-live",
    render: () => <LiveRoom change="gone" />,
  },
];
