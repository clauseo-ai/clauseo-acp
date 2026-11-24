// The 24 Nov plan-low entries (UC-1170): the composer's warning before sending, for the weekly window at 92% used, its
// reset on another day, in the real room over FakeTransport with the plan's limits on its channel. The 5-hour window's
// words keep their U12 ids (U12-24 to U12-26, registry-u12.tsx), as does the running-low line by the composer (U15-19).
import type { Entry } from "./entry.ts";
import { figures, warning, weekly } from "./registry-u12.tsx";
import { limitThreadWords } from "./words.ts";

export const UC_PLAN_LOW: Entry[] = [
  warning(
    "UC-1170",
    limitThreadWords("You've used {n}% of your weekly {name} limit."),
    figures(30, { windows: [figures(30).windows[0]!, weekly(92)] }),
  ),
];
