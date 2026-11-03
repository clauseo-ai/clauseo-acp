// The plan's figures as the room says them (U12), pure: the toolbar's line by the share used, the share left, and
// whether the composer warns. Thresholds from HANDOFF item 13: plenty under 50% used, under half left under 75%, close
// to the limit from 75% (or when the agent warns, tone `near`), the limit reached at 100% (tone `at_limit`). The share
// left is 100 minus used, rounded down. Times are clock times against `now`, the render's own today: never a span.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lim, type LimitKey } from "@clauseo/components/lib/words-limits.ts";
import type { PlanState } from "@clauseo/components/ui/meter.tsx";
import type {
  AgentKind,
  AgentLimitsView,
  LimitMeter,
  Timestamp,
} from "../../transport/protocol.ts";
import { stamp, whenOf } from "../thread/clock.ts";

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

/** The at-limit words with the reset, today's time or another day's; the no-reset form without one. */
export function reachedLine(lang: Lang, resets: Timestamp | undefined, now: number): string {
  if (resets === undefined) return lim(lang, "Limit reached. Reset not given.");
  const { day, time } = whenOf(lang, resets, now);
  return day === null
    ? lim(lang, "Limit reached until {time}", { time })
    : lim(lang, "Limit reached until {day} {time}", { day, time });
}

const WITH_RESET: Record<Exclude<Band, "reached">, [LimitKey, LimitKey]> = {
  plenty: ["Plenty left until the {time} reset", "Plenty left"],
  half: ["Under half left. Resets at {time}.", "Under half left."],
  close: ["Close to the limit. Resets at {time}.", "Close to the limit."],
};

/** The toolbar's line for a meter. */
export function meterLine(lang: Lang, meter: LimitMeter, now: number): string {
  const band = bandOf(meter);
  if (band === "reached") return reachedLine(lang, meter.resets_at, now);
  const [timed, bare] = WITH_RESET[band];
  return meter.resets_at === undefined
    ? lim(lang, bare)
    : lim(lang, timed, { time: stamp(lang, meter.resets_at, now) });
}

/** The toolbar's plan for an entry, as RoomToolbar takes it, without the card: null where no line stands. */
export type PlanLine = {
  name: string;
  left: number;
  valueText: string;
  status: string;
  state: PlanState;
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
  return {
    name,
    left: bandOf(meter) === "reached" ? 0 : left / 100,
    valueText: lim(lang, "{n} percent of the plan left", { n: left }),
    status: meterLine(lang, meter, now),
    state: STATE[bandOf(meter)],
    ...(look === "stale" ? { look: "stale" as const } : {}),
    tone: meter.tone,
    freshness: entry.freshness,
  };
}

/** The composer's warning, as one string so a part renders again only when it changes: the window's label and its
 * reset, "" for none. A session window at 95% used or more, or the agent's own warning (tone `near` under 75% used,
 * which the share alone never gives). Never for an account billed by use, and never "close to" a limit already reached:
 * the toolbar's meter at its limit, or the session window at 100%, where the toolbar says Limit reached. */
export function warningKey(entry: AgentLimitsView | undefined): string {
  if (entry === undefined || lookOf(entry) === "payg") return "";
  const meter = entry.meter;
  if (meter !== undefined && bandOf(meter) === "reached") return "";
  const session = entry.windows.find(
    (w) =>
      w.kind === "session" &&
      w.used_percent !== undefined &&
      w.used_percent >= 95 &&
      w.used_percent < 100 &&
      w.tone !== "at_limit",
  );
  const hit =
    session !== undefined
      ? { label: session.label, resets: session.resets_at }
      : meter !== undefined && meter.tone === "near" && meter.used_percent < 75
        ? { label: meter.window_label, resets: meter.resets_at }
        : null;
  return hit === null ? "" : JSON.stringify([hit.label, hit.resets ?? null]);
}
