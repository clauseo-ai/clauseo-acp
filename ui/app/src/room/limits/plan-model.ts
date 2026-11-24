// The plan's figures as the room says them (U12), pure: the toolbar's line by the share used, the share left, and
// whether the composer warns. Thresholds from HANDOFF item 13: plenty under 50% used, under half left under 75%, close
// to the limit from 75% (or when the agent warns, tone `near`), the limit reached at 100% (tone `at_limit`). The share
// left is 100 minus used, rounded down. Times are clock times against `now`, the render's own today: never a span.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lim, type LimitKey } from "@clauseo/components/lib/words-limits.ts";
import type { PlanLevel, PlanState } from "@clauseo/components/ui/meter.tsx";
import type { AgentKind, AgentLimitsView, LimitMeter, SignIn } from "../../transport/protocol.ts";
import { stamp } from "../thread/clock.ts";
// the at-limit words, said once for the toolbar and the paused composer, both on the first screen
import { atLimitWords } from "../thread/limit-words.ts";

/** Each kind's plan, as the toolbar names it. */
export const PLAN_NAME: Record<AgentKind, string> = { claude: "Claude", codex: "ChatGPT" };

/** The share of a window left, 0 to 100: 100 minus used, rounded down. */
export const shareLeft = (used: number): number =>
  Math.min(100, Math.max(0, Math.floor(100 - used)));

/** Where the toolbar's line stands. */
export type Band = "plenty" | "half" | "close" | "reached";

export function bandOf(meter: LimitMeter): Band {
  if (meter.tone === "at_limit" || meter.used_percent >= 100) return "reached";
  if (meter.tone === "near" || meter.used_percent >= 75) return "close";
  return meter.used_percent >= 50 ? "half" : "plenty";
}

/** The meter's fill as the plan runs short (24 Nov ruling): red at 5% left or less, by the share left the figures
 * show (shareLeft, rounded down, so 94.5% used, "5% left", is red), or at the limit; amber from the close band on (75%
 * used, or the agent's own warning); none below. */
export function levelOf(meter: LimitMeter): PlanLevel | undefined {
  const band = bandOf(meter);
  if (band === "reached" || shareLeft(meter.used_percent) <= 5) return "red";
  return band === "close" ? "amber" : undefined;
}

const STATE: Record<Band, PlanState> = {
  plenty: "plenty",
  half: "plenty",
  close: "close",
  reached: "reached",
};

/** What the room shows for an agent's entry: no line for the states whose words wait on QU.8 (missing_scope,
 * live_only) and one from a newer Clauseo; else the line's look. */
export type PlanLook = "figures" | "stale" | "loading" | "none" | "payg" | "hidden";

export function lookOf(entry: AgentLimitsView): PlanLook {
  if (entry.billing === "pay_as_you_go" || entry.freshness === "not_applicable") return "payg";
  switch (entry.freshness) {
    case "fresh":
      return "figures";
    case "stale":
      return "stale";
    case "loading":
      return "loading";
    case "unavailable":
      return "none";
    default:
      return "hidden";
  }
}

/** The name the toolbar and the card stand under: "Claude" (the toolbar adds "plan"), or billed by use, "Claude, pay
 * as you go". */
export function planTitle(lang: Lang, kind: AgentKind, look: PlanLook): string {
  const name = PLAN_NAME[kind];
  return look === "payg" ? lim(lang, "{name}, pay as you go", { name }) : name;
}

const WITH_RESET: Record<Exclude<Band, "reached">, [LimitKey, LimitKey]> = {
  plenty: ["Plenty left until the {time} reset", "Plenty left"],
  half: ["Under half left. Resets at {time}.", "Under half left."],
  close: ["Close to the limit. Resets at {time}.", "Close to the limit."],
};

/** The toolbar's line for a meter. */
export function meterLine(lang: Lang, meter: LimitMeter, now: number): string {
  const band = bandOf(meter);
  if (band === "reached") return atLimitWords(lang, meter.resets_at, now);
  const [timed, bare] = WITH_RESET[band];
  return meter.resets_at === undefined
    ? lim(lang, bare)
    : lim(lang, timed, { time: stamp(lang, meter.resets_at, now) });
}

/** How far `resets_at` may move from the reset the line showed last and still show that one: the host's reads land a
 * little apart, so a reset jitters by a few hundred ms, and at a minute's edge that flips the minute shown. */
const RESET_SLACK_MS = 30_000;
/** The reset instant each window's line last showed, by agent kind and window label. */
const shownResets = new Map<string, number>();

/** The reset to show for a window: the one it showed last, unless `resets_at` is 30 s or more away from it, which is
 * then the one shown from here on. */
function shownReset(key: string, resets_at: number): number {
  const held = shownResets.get(key);
  if (held !== undefined && Math.abs(resets_at - held) < RESET_SLACK_MS) return held;
  shownResets.set(key, resets_at);
  return resets_at;
}

/** Forgets every reset the lines showed (the tests start each from none). */
export function forgetShownResets(): void {
  shownResets.clear();
}

/** The toolbar's plan for an entry, as RoomToolbar takes it, without the card: null where no line stands. */
export type PlanLine = {
  name: string;
  left: number;
  valueText: string;
  status: string;
  state: PlanState;
  level?: PlanLevel | undefined;
  look?: "stale" | "none" | "bare" | "loading";
  tone?: string;
  freshness: string;
};

export function planLine(
  lang: Lang,
  kind: AgentKind,
  entry: AgentLimitsView,
  now: number,
): PlanLine | null {
  const look = lookOf(entry);
  if (look === "hidden") return null;
  const name = planTitle(lang, kind, look);
  const base = {
    name,
    left: 0,
    valueText: "",
    state: "plenty" as const,
    freshness: entry.freshness,
  };
  if (look === "loading") return { ...base, status: "", look: "loading" };
  if (look === "payg")
    return {
      ...base,
      status: entry.provider === "anthropic" ? lim(lang, "Billed to your Anthropic account") : "",
      look: "bare",
    };
  if (look === "none")
    return { ...base, status: lim(lang, "Usage not shown right now"), look: "none" };
  const meter = entry.meter;
  // figures with no window to show: the name alone, as while loading, with the card still to open
  if (meter === undefined) return { ...base, status: "", look: "none" };
  const left = shareLeft(meter.used_percent);
  const resets =
    meter.resets_at === undefined
      ? meter
      : { ...meter, resets_at: shownReset(`${kind}:${meter.window_label}`, meter.resets_at) };
  return {
    name,
    left: bandOf(meter) === "reached" ? 0 : left / 100,
    valueText: lim(lang, "{n} percent of the plan left", { n: left }),
    status: meterLine(lang, resets, now),
    state: STATE[bandOf(meter)],
    level: levelOf(meter),
    ...(look === "stale" ? { look: "stale" as const } : {}),
    tone: meter.tone,
    freshness: entry.freshness,
  };
}

/** Whether the header may name an agent's plan and who bills it, from the sign-in the setup store
 * holds for it: signed in, by a plan login, a key or the firm's endpoint, whether or not the
 * sign-in names the plan (the limits' line then stands as it is). Signed out, signing in,
 * switching, failed, or no sign-in heard yet: false, and the header names the agent alone. */
export function planNamedBy(signIn: SignIn | undefined): boolean {
  return typeof signIn === "object" && signIn.signed_in !== undefined;
}

/** The header's line for an agent not signed in: its name alone, with no meter, no status and no
 * card. */
export function nameOnlyLine(kind: AgentKind, freshness: string): PlanLine {
  return {
    name: PLAN_NAME[kind],
    left: 0,
    valueText: "",
    status: "",
    state: "plenty",
    look: "bare",
    freshness,
  };
}

/** The windows the composer warns of before a question is sent, and from what share used: the 5-hour (session) window
 * from 75%, the weekly from 90% (24 Nov ruling: one question took a Pro plan's 5 hours from 27% to 73%). */
export const WARN_FROM = { session: 75, weekly: 90 } as const;

/** What the composer's warning names: the 5-hour window, the weekly one, or another (the agent's own warning, or a
 * session window by another name), with the share used as the card counts it (100 less the share left), never more
 * than 99: below the limit the note never says 100%. */
export type WarningWindow = "five_hour" | "weekly" | "other";
export type Warning = { window: WarningWindow; used: number; resets: number | null };

/** The share used the note says: 100 less the share left the card shows, capped at 99 below the limit. */
const usedShown = (used: number): number => Math.min(99, 100 - shareLeft(used));

/** The composer's warning, as one string so a part renders again only when it changes: JSON of a Warning, "" for none.
 * The 5-hour window from 75% used and the weekly from 90%, below 100; both high, the one with less left (a tie names
 * the 5-hour). Else the agent's own warning (tone `near` under 75% used, which the share alone never gives). Never for
 * an account billed by use, and never "close to" a limit already reached: the toolbar's meter at its limit, where the
 * toolbar says Limit reached. */
export function warningKey(entry: AgentLimitsView | undefined): string {
  if (entry === undefined || lookOf(entry) === "payg") return "";
  const meter = entry.meter;
  if (meter !== undefined && bandOf(meter) === "reached") return "";
  let hit: Warning | null = null;
  for (const w of entry.windows) {
    if (w.kind !== "session" && w.kind !== "weekly") continue;
    const used = w.used_percent;
    if (used === undefined || used < WARN_FROM[w.kind] || used >= 100 || w.tone === "at_limit")
      continue;
    const share = usedShown(used);
    // both high: the one with less left; a tie names the 5-hour
    if (hit !== null && (share < hit.used || (share === hit.used && w.kind === "weekly"))) continue;
    const window: WarningWindow =
      w.kind === "weekly" ? "weekly" : w.label === "5-hour limit" ? "five_hour" : "other";
    hit = { window, used: share, resets: w.resets_at ?? null };
  }
  if (hit === null && meter !== undefined && meter.tone === "near" && meter.used_percent < 75)
    hit = {
      window: "other",
      used: usedShown(meter.used_percent),
      resets: meter.resets_at ?? null,
    };
  return hit === null ? "" : JSON.stringify(hit);
}
