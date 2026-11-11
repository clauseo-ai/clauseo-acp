// The Smart App Control note's copy review entries (SAC-01 on): the room on Windows whose display_info says Smart App
// Control is still in evaluation, the note at the top of the work column, before "Got it". "Got it" itself is UD2-07's.
import { FakeTransport } from "../transport/fake-transport.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { roomWords } from "./words.ts";

const KEPT = "clauseo.smart-app-control-noted";

/** A Windows host in evaluation, with the note not yet put away on this page (the frames share one localStorage). */
const evaluating = (): FakeTransport => {
  localStorage.removeItem(KEPT);
  return new FakeTransport({
    platform: "windows",
    smartAppControl: "evaluation",
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      throw new Error(`the review's host answers list_matters only, not ${JSON.stringify(query)}`);
    },
  });
};

const render = () => <RoomOver host={evaluating} />;

export const SAC: Entry[] = [
  {
    id: "SAC-01",
    words: roomWords("If Windows ever blocks Clauseo, turn off Smart App Control."),
    screen: "room/sac-note",
    render,
  },
  { id: "SAC-02", words: roomWords("How to do this"), screen: "room/sac-note", render },
];
