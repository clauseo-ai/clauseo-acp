// States the review opens by id (?cell=<id>, cell.tsx) that register no string, so the copy gate never reads them:
// for a look at a part whose words are registered where they first show.
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import type { Entry } from "./entry.ts";
import { LowMidTurn } from "./room-low-credit.tsx";
import { PLAN_LOW_SCENES } from "./scenes-plan-low.tsx";

const LOW = "Running low. Add credits to keep searching.";

export const SCENES: readonly Entry[] = [
  {
    // U15-19's words, and U15-66's Add credits, under a turn in the room
    id: "room/low-credit",
    words: {
      file: "lib/words-connectors.ts",
      key: LOW,
      text: (lang) => cw(lang, LOW),
      later: false,
    },
    screen: "room/low-credit",
    render: () => <LowMidTurn />,
  },
  ...PLAN_LOW_SCENES,
];
