// Every string the copy review shows, one entry each. A brief that adds or changes a string adds its entry here: the
// next free id for the brief ("U1-04"), the words file and English key (words.ts's helpers), the screen, the state
// that shows it (the real screen over a scripted host, built in the render function), and, where the default desktop
// window (entry.ts) doesn't suit, the width and height to render at. Approvals live in ui/app/copy-approvals.json.
import type { Entry } from "./entry.ts";
import { MATTER, roomHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { frameWords, shared } from "./words.ts";

export const REGISTRY: readonly Entry[] = [
  {
    id: "U1-01",
    words: shared("Matters"),
    screen: "room/toolbar",
    // the toolbar and the top of the room under it
    height: 200,
    render: () => <RoomOver host={() => roomHost([MATTER])} />,
  },
  {
    id: "U1-02",
    words: frameWords("No matters yet"),
    screen: "room/toolbar",
    // the toolbar and the top of the room under it
    height: 200,
    render: () => <RoomOver host={() => roomHost([])} />,
  },
  {
    id: "U1-03",
    words: shared("No papers yet."),
    screen: "room/wall",
    render: () => <RoomOver host={() => roomHost([MATTER], [])} />,
  },
];
