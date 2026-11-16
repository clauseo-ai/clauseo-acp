// Latest's copy review entries (thread.tsx, follow.ts): the visible word and the button's name share one state, the
// room over a long ChatGPT answer that streams in and is then scrolled back up (room-following.tsx).
import type { Entry } from "./entry.ts";
import { RoomFollowing } from "./room-following.tsx";
import { threadWords } from "./words.ts";

/** Latest's entries, each id written as it is. */
export const TF: Entry[] = [
  {
    id: "TF-01",
    words: threadWords("Latest"),
    screen: "room/latest",
    render: () => <RoomFollowing />,
  },
  {
    id: "TF-02",
    words: threadWords("Go to the latest message"),
    screen: "room/latest",
    render: () => <RoomFollowing />,
  },
];
