// The 24 Nov plan-low entries (UC-1170 to UC-1176), in the real room over FakeTransport with the plan's limits on its
// channel: the composer's warning before sending, for the 5-hour window at 78% (its reset today, another day, or none
// given) and the weekly window at 92%; and Settings' side note naming each plan. The words they replaced keep their
// approved U12 ids (U12-24 to U12-26, U12-38, U12-39) until the sweep retires them. The running-low line by the
// composer reuses U15-19's words.
import type { Entry } from "./entry.ts";
import { BOTH, figures, LATER_RESET, PAYG, row, usage, warning, weekly } from "./registry-u12.tsx";
import { limitThreadWords, settingsWords } from "./words.ts";

const { resets_at: _, ...noReset } = row(78);

export const UC_PLAN_LOW: Entry[] = [
  warning(
    "UC-1170",
    limitThreadWords("You've used {n}% of your weekly {name} limit."),
    figures(30, { windows: [row(30), weekly(92)] }),
  ),
  warning(
    "UC-1171",
    limitThreadWords("You've used {n}% of your 5-hour {name} limit."),
    figures(78),
  ),
  warning(
    "UC-1172",
    limitThreadWords(
      "It resets at {time}. If it runs out mid-answer, the answer pauses until then.",
    ),
    figures(78),
  ),
  warning(
    "UC-1173",
    limitThreadWords(
      "It resets on {day} at {time}. If it runs out mid-answer, the answer pauses until then.",
    ),
    figures(78, { windows: [row(78, { resets_at: LATER_RESET }), weekly(20)] }),
  ),
  warning(
    "UC-1174",
    limitThreadWords("If it runs out mid-answer, the answer pauses until it resets."),
    figures(78, { windows: [noReset, weekly(20)] }),
  ),
  usage("UC-1175", settingsWords("{name}: {n}% left"), BOTH),
  usage("UC-1176", settingsWords("{name}: pay as you go"), { claude: PAYG }),
];
