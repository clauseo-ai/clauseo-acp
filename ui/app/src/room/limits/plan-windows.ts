// A plan's windows as the parts say them (U12), pure, shared by the room's card (plan-card-view.tsx) and Settings'
// Plan and usage: each window's label in the language, its share left (none while loading or once its reset passed, so
// an old figure never reads as current), its reset as a clock time, and whether it is close to the limit, by the same
// band as the toolbar's line. Loaded with the card, never with the room's first screen: the card's words come with it.
import type { PlanWindow } from "@clauseo/components/clauseo/plan-card.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { pc, type PlanCardKey } from "@clauseo/components/lib/words-plan-card.ts";
import type { LimitWindowView } from "../../transport/protocol.ts";
import { whenOf } from "../thread/clock.ts";
import { bandOf, shareLeft } from "./plan-model.ts";

/** The window labels the card's words translate; another shows as the host gives it, in English. */
const LABELS = new Set<string>([
  "5-hour limit",
  "Weekly limit",
  "Opus weekly limit",
  "Sonnet weekly limit",
  "Weekly limit for connected apps",
  "Monthly limit",
  "Usage limit",
]);

export const windowLabel = (lang: Lang, label: string): string =>
  LABELS.has(label) ? pc(lang, label as PlanCardKey) : label;

/** A window's reset: "Resets 6:10 pm" ahead, "The limit reset at 4:10 pm." once passed; "" without one. */
export function resetOf(lang: Lang, w: LimitWindowView, now: number): string {
  if (w.resets_at === undefined) return "";
  const { day, time } = whenOf(lang, w.resets_at, now);
  if (w.freshness === "reset_passed")
    return day === null
      ? lim(lang, "The limit reset at {time}.", { time })
      : lim(lang, "The limit reset on {day} at {time}.", { day, time });
  return day === null
    ? lim(lang, "Resets {time}", { time })
    : lim(lang, "Resets {day} {time}", { day, time });
}

/** A window as the parts draw it: no share (and so no bar) while loading or once its reset passed; close from the
 * toolbar's close band on (75% used, or the agent's own warning). */
export type WindowFigures = PlanWindow & { close: boolean; passed: boolean };

export function windowOf(lang: Lang, w: LimitWindowView, now: number): WindowFigures {
  const figures = w.freshness !== "reset_passed" && w.freshness !== "loading";
  const left = figures && w.used_percent !== undefined ? shareLeft(w.used_percent) : null;
  const band =
    left === null || w.used_percent === undefined
      ? null
      : bandOf({ tone: w.tone, used_percent: w.used_percent, window_label: w.label });
  return {
    name: windowLabel(lang, w.label),
    left,
    leftText: left === null ? "" : pc(lang, "{n}% left", { n: left }),
    reset: resetOf(lang, w, now),
    valueText: left === null ? "" : lim(lang, "{n} percent of the plan left", { n: left }),
    close: band === "close" || band === "reached",
    passed: w.freshness === "reset_passed",
  };
}
