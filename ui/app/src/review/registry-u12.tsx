// U12's copy review entries (the usage limit), apart from registry.tsx to keep that file under the size limit: each
// string in the real room over FakeTransport, the plan's limits played on its channel as `limits_view_changed`: the
// toolbar's plan at each band, with and without a reset; its card opened (fresh, stale, none, pay as you go, and every
// window label the card knows, one of them past its reset); the composer's warning; the stop at the plan's limit (the
// 5-hour window, the weekly one, and a reset not given, with and without the stop's time); the composer while paused;
// and Settings' Plan and usage with its note in the side list. The paused rail's words are U2's entries (U2-07 to
// U2-09).
import type {
  AgentLimitsView,
  LimitMeter,
  LimitsView,
  LimitWindowView,
  MeterTone,
  ThreadStatus,
  ViewEntry,
} from "../transport/protocol.ts";
import { agentText, step, TURN, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomClicking, RoomOver } from "./states.tsx";
import { SettingsOver } from "./states-settings.tsx";
import {
  limitThreadWords,
  limitWords,
  planCardWords,
  settingsWords,
  type WordsRef,
} from "./words.ts";

/** A local time, `days` from today. */
const atLocal = (days: number, h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days, h, m).getTime();
};
/** Today's reset, late enough to stay ahead of the reviewer's clock; another day's; the read at 4:29 pm. */
const RESET = atLocal(0, 23, 50);
const LATER_RESET = atLocal(2, 9, 0);
const READ = atLocal(0, 16, 29);
const ASKED = atLocal(0, 16, 31);
const STOPPED = atLocal(0, 16, 52);

const QUESTION: ViewEntry = (() => {
  const asked = userMessage(5, 1, "Draft the reply to the bank's notice under clause nine.");
  return { user_message: { ...asked.user_message!, ts: ASKED } };
})();
const ANSWER = agentText(
  6,
  1,
  "The reply says the notice was served late, and asks the bank to withdraw it.",
);
const PARTIAL = agentText(6, 1, "The reply says the notice was served late, and");
const ENDED = step(9, { ts: atLocal(0, 16, 33) });

const toneOf = (used: number): MeterTone =>
  used >= 100 ? "at_limit" : used >= 75 ? "near" : "neutral";

const row = (used: number | undefined, over: Partial<LimitWindowView> = {}): LimitWindowView => ({
  id: "five_hour",
  kind: "session",
  label: "5-hour limit",
  ...(used === undefined ? {} : { used_percent: used }),
  tone: toneOf(used ?? 0),
  resets_at: RESET,
  freshness: "fresh",
  ...over,
});
const weekly = (used: number) =>
  row(used, { id: "seven_day", kind: "weekly", label: "Weekly limit", resets_at: LATER_RESET });

/** The plan's meter at `used`, its reset given unless `resets` is null. */
const meter = (used: number, resets: number | null = RESET): LimitMeter => ({
  tone: toneOf(used),
  used_percent: used,
  window_label: "5-hour limit",
  ...(resets === null ? {} : { resets_at: resets }),
});

/** Claude Max's figures at `used` of the 5-hour window, the weekly one at 20%. */
const figures = (used: number, over: Partial<AgentLimitsView> = {}): AgentLimitsView => ({
  plan: "Claude Max",
  billing: "plan",
  meter: meter(used),
  windows: [row(used), weekly(20)],
  at_limit_text: "",
  freshness: "fresh",
  checked_at: READ,
  ...over,
});
/** An entry with no figures. */
const bare = (over: Partial<AgentLimitsView>): AgentLimitsView => {
  const { meter: _, ...e } = figures(0, { windows: [], ...over });
  return e;
};
const NONE = bare({ freshness: "unavailable" });
const PAYG = bare({ freshness: "not_applicable", billing: "pay_as_you_go", provider: "anthropic" });

/** Every label the card says in its own words, one past its reset (no share, no bar, its reset muted). */
const EVERY_WINDOW = figures(62, {
  windows: [
    row(62),
    weekly(20),
    row(41, {
      id: "seven_day_opus",
      kind: "model",
      label: "Opus weekly limit",
      resets_at: LATER_RESET,
    }),
    row(12, {
      id: "seven_day_sonnet",
      kind: "model",
      label: "Sonnet weekly limit",
      resets_at: LATER_RESET,
    }),
    row(5, {
      id: "seven_day_oauth_apps",
      kind: "weekly",
      label: "Weekly limit for connected apps",
      resets_at: LATER_RESET,
    }),
    row(30, { id: "monthly", kind: "other", label: "Monthly limit", resets_at: LATER_RESET }),
    row(undefined, {
      id: "usage",
      kind: "other",
      label: "Usage limit",
      freshness: "reset_passed",
      resets_at: atLocal(0, 0, 1),
    }),
  ],
});

/** The open conversation, its answer ended, with the plan's limits on the channel. */
const withPlan = (limits: LimitsView, over: Partial<ThreadState> = {}): ThreadState => ({
  entries: [QUESTION, ANSWER, ENDED],
  limits,
  ...over,
});

const toolbar = (id: string, words: WordsRef, claude: AgentLimitsView): Entry => ({
  id,
  words,
  screen: "room/toolbar",
  render: () => <RoomOver host={() => threadHost(withPlan({ claude }))} />,
});

/** The plan's card, opened from the toolbar. */
const card = (
  id: string,
  words: WordsRef,
  screen: string,
  claude: AgentLimitsView,
  height = 700,
): Entry => ({
  id,
  words,
  screen,
  height,
  render: () => (
    <RoomClicking host={() => threadHost(withPlan({ claude }))} clicks={['[data-testid="plan"]']} />
  ),
});

const warning = (id: string, words: WordsRef, claude: AgentLimitsView): Entry => ({
  id,
  words,
  screen: "limits/warning",
  render: () => <RoomOver host={() => threadHost(withPlan({ claude }))} />,
});

/** The stop at the plan's limit for `window`, at `ts` (none: the stop's time not known), the conversation paused
 * until `resets` (none: not given). */
const stopped = (
  window: string | undefined,
  resets: number | undefined,
  ts: number | undefined,
): ThreadState => {
  const entry: ViewEntry = {
    limit_stop: {
      order: 9,
      turn: TURN,
      reason: "plan_window",
      ...(window === undefined ? {} : { window }),
      ...(resets === undefined ? {} : { resets_at: resets }),
      ...(ts === undefined ? {} : { ts }),
    } as never,
  };
  const status: ThreadStatus = {
    paused_at_limit: resets === undefined ? {} : { resets_at: resets },
  };
  return { entries: [QUESTION, PARTIAL, entry], status, limits: { claude: figures(100) } };
};

const stop = (id: string, words: WordsRef, screen: string, state: ThreadState): Entry => ({
  id,
  words,
  screen,
  render: () => <RoomOver host={() => threadHost(state)} />,
});

/** The settings window at Plan and usage, 960 by 700 (DESIGN.md). */
const usage = (id: string, words: WordsRef, limits: LimitsView): Entry => ({
  id,
  words,
  screen: "settings/usage",
  width: 960,
  height: 700,
  render: (lang) => <SettingsOver section="usage" lang={lang} limits={limits} />,
});

/** Claude with figures, ChatGPT with its own. */
const BOTH: LimitsView = {
  claude: figures(62),
  codex: figures(30, { plan: "ChatGPT Plus", windows: [row(30), weekly(10)] }),
};

/** U12's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U12: Entry[] = [
  // the toolbar's line at each band, with the reset and without one
  toolbar("U12-01", limitWords("Plenty left until the {time} reset"), figures(30)),
  toolbar("U12-02", limitWords("Under half left. Resets at {time}."), figures(60)),
  toolbar("U12-03", limitWords("Close to the limit. Resets at {time}."), figures(80)),
  toolbar("U12-04", limitWords("Plenty left"), figures(30, { meter: meter(30, null) })),
  toolbar("U12-05", limitWords("Under half left."), figures(60, { meter: meter(60, null) })),
  toolbar("U12-06", limitWords("Close to the limit."), figures(80, { meter: meter(80, null) })),
  toolbar("U12-07", limitWords("Usage not shown right now"), NONE),
  toolbar("U12-08", limitWords("{name}, pay as you go"), PAYG),
  toolbar("U12-09", limitWords("Billed to your Anthropic account"), PAYG),
  // the meter's value for a screen reader, and the plan's name as a button
  toolbar("U12-10", limitWords("{n} percent of the plan left"), figures(30)),
  toolbar("U12-11", limitWords("Show the plan's limits"), figures(30)),
  // the card, every label it knows, one window past its reset
  card("U12-12", planCardWords("5-hour limit"), "limits/plan-card", EVERY_WINDOW, 900),
  card("U12-13", planCardWords("Weekly limit"), "limits/plan-card", EVERY_WINDOW, 900),
  card("U12-14", planCardWords("Opus weekly limit"), "limits/plan-card", EVERY_WINDOW, 900),
  card("U12-15", planCardWords("Sonnet weekly limit"), "limits/plan-card", EVERY_WINDOW, 900),
  card(
    "U12-16",
    planCardWords("Weekly limit for connected apps"),
    "limits/plan-card",
    EVERY_WINDOW,
    900,
  ),
  card("U12-17", planCardWords("Monthly limit"), "limits/plan-card", EVERY_WINDOW, 900),
  card("U12-18", planCardWords("Usage limit"), "limits/plan-card", EVERY_WINDOW, 900),
  card("U12-19", planCardWords("{n}% left"), "limits/plan-card", figures(62)),
  // stale: the figures muted
  card(
    "U12-20",
    planCardWords("As of {time}"),
    "limits/plan-stale",
    figures(62, { freshness: "stale" }),
  ),
  // none, with Try again
  card("U12-21", planCardWords("Usage isn't available right now."), "limits/plan-none", NONE),
  card(
    "U12-22",
    planCardWords(
      "Your {name} plan didn't send its limits this time. Asking still works as usual.",
    ),
    "limits/plan-none",
    NONE,
  ),
  card(
    "U12-23",
    planCardWords("Pay as you go, billed to your Anthropic account"),
    "limits/plan-payg",
    PAYG,
  ),
  // the composer's warning: the 5-hour window at 96%, its reset today or another day
  warning("U12-24", limitThreadWords("Close to your 5-hour limit."), figures(96)),
  warning("U12-25", limitThreadWords("Long tasks may pause until {time}."), figures(96)),
  warning(
    "U12-26",
    limitThreadWords("Long tasks may pause until {day} {time}."),
    figures(96, { windows: [row(96, { resets_at: LATER_RESET }), weekly(20)] }),
  ),
  // the stop: the 5-hour window (hit), the weekly one, a reset not given (unknown); with the stop's time and without
  stop(
    "U12-27",
    limitThreadWords("{name} plan limit reached at {time}"),
    "limits/hit",
    stopped("five_hour", RESET, STOPPED),
  ),
  stop(
    "U12-28",
    limitThreadWords("{name} plan limit reached"),
    "limits/unknown",
    stopped(undefined, undefined, undefined),
  ),
  stop(
    "U12-29",
    limitThreadWords("The 5-hour limit. It resets at {time}."),
    "limits/hit",
    stopped("five_hour", RESET, STOPPED),
  ),
  stop(
    "U12-30",
    limitThreadWords("The 5-hour limit. It resets on {day} at {time}."),
    "limits/hit",
    stopped("five_hour", LATER_RESET, undefined),
  ),
  stop(
    "U12-31",
    limitThreadWords("The weekly limit. It resets at {time}."),
    "limits/weekly",
    stopped("seven_day", RESET, STOPPED),
  ),
  stop(
    "U12-32",
    limitThreadWords("The weekly limit. It resets on {day} at {time}."),
    "limits/weekly",
    stopped("seven_day", LATER_RESET, STOPPED),
  ),
  stop(
    "U12-33",
    limitThreadWords("Stopped here at {time}"),
    "limits/hit",
    stopped("five_hour", RESET, STOPPED),
  ),
  // stopped two days ago, its reset two days on
  stop(
    "U12-34",
    limitThreadWords("Stopped here on {day} at {time}"),
    "limits/hit",
    stopped("five_hour", LATER_RESET, atLocal(-2, 16, 52)),
  ),
  // the composer while paused, the stop's time known; without it, the toolbar's at-limit words (U2-07 to U2-09)
  stop(
    "U12-35",
    limitWords("Stopped at {time}, at the plan's limit"),
    "room/composer",
    stopped("five_hour", RESET, STOPPED),
  ),
  stop(
    "U12-36",
    limitWords("Stopped on {day} at {time}, at the plan's limit"),
    "room/composer",
    stopped("five_hour", LATER_RESET, atLocal(-2, 16, 52)),
  ),
  // Settings' Plan and usage, its note in the side list
  usage("U12-37", settingsWords("Plan and usage"), BOTH),
  usage("U12-38", settingsWords("{n}% of the 5-hour limit left"), BOTH),
  usage("U12-39", settingsWords("Pay as you go"), { claude: PAYG }),
  usage("U12-40", settingsWords("What each plan has left, and when it resets."), BOTH),
  usage("U12-41", settingsWords("Updated at {time}"), BOTH),
  usage(
    "U12-42",
    settingsWords("No limits to show. Each question is billed to the key's account."),
    {
      claude: PAYG,
    },
  ),
];
