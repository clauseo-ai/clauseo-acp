// Every string the copy review shows, one entry each. A brief that adds or changes a string adds its entry here: the
// next free id for the brief ("U1-04"), the words file and English key (words.ts's helpers), the screen, and the state
// that shows it (states.tsx). Approvals live in ui/app/copy-approvals.json, by id.
import { frame } from "@clauseo/components/lib/words-frame.ts";
import type { Entry } from "./entry.ts";
import { RoomFrameToolbar, RoomFrameWall } from "./states.tsx";
import { frameWords, shared } from "./words.ts";

/** A matter's name as filed, for states that show one. */
const MATTER = "Acme Traders v. State";

export const REGISTRY: readonly Entry[] = [
  {
    id: "U1-01",
    words: shared("Matters"),
    screen: "room/toolbar",
    render: () => <RoomFrameToolbar title={MATTER} />,
  },
  {
    id: "U1-02",
    words: frameWords("No matters yet"),
    screen: "room/toolbar",
    render: (lang) => <RoomFrameToolbar title={frame(lang, "No matters yet")} />,
  },
  {
    id: "U1-03",
    words: shared("No papers yet."),
    screen: "room/wall",
    render: () => <RoomFrameWall />,
  },
];
