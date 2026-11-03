// The plan's card, opened from the plan in the toolbar (U12): "As of {time}", then every window with its share left,
// its bar and its reset, from the host's limits view. A window whose reset passed (the host reads again) shows no bar
// and no share, only when it reset, muted. With no figures the card says so and offers Try again (`refresh_limits`,
// `try_again`); billed by use, it has its title and who bills, no bar and no figure. Its own chunk: only a lawyer who
// opens the card needs it. Not here until the host gives them (H16, H11): the stale line's retry time, the month's
// figure, what happens at the limit, Manage plan.
import { PlanCard, type PlanWindow } from "@clauseo/components/clauseo/plan-card.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useAgentLimits, useLimitsAt, useLimitsStore } from "../../store/limits.ts";
import type { AgentKind, LimitWindowView } from "../../transport/protocol.ts";
import { stamp, whenOf } from "../thread/clock.ts";
import { PLAN_NAME, lookOf, planTitle, shareLeft, windowLabel } from "./plan-model.ts";

/** A window's reset: "Resets 6:10 pm" ahead, "The limit reset at 4:10 pm." once passed; "" without one. */
function resetOf(lang: Lang, w: LimitWindowView, now: number): string {
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

/** A window as the card draws it: no share (and so no bar) while loading or once its reset passed. */
function windowOf(lang: Lang, w: LimitWindowView, now: number): PlanWindow {
  const figures = w.freshness !== "reset_passed" && w.freshness !== "loading";
  const left = figures && w.used_percent !== undefined ? shareLeft(w.used_percent) : null;
  return {
    name: windowLabel(lang, w.label),
    left,
    leftText: left === null ? "" : lim(lang, "{n}% left", { n: left }),
    reset: resetOf(lang, w, now),
    valueText: left === null ? "" : lim(lang, "{n} percent of the plan left", { n: left }),
  };
}

export function LivePlanCard({ kind }: { kind: AgentKind }) {
  const w = useWords();
  const entry = useAgentLimits(kind);
  const store = useLimitsStore();
  // times against the view's arrival: the host sends a new view at each shown reset, nothing ticks here
  const now = useLimitsAt();
  if (entry === undefined) return null;
  const { lang } = w;
  const look = lookOf(entry);
  const figures = look === "figures" || look === "stale";
  const name = PLAN_NAME[kind];
  const message =
    look === "none"
      ? {
          title: lim(lang, "Usage isn't available right now."),
          body: lim(
            lang,
            "Your {name} plan didn't send its limits this time. Asking still works as usual.",
            { name },
          ),
          action: (
            <Button
              kind="tone"
              size="small"
              className="plan-card-try"
              data-testid="plan-try-again"
              onClick={() => store?.refresh(kind, "try_again")}
            >
              {lim(lang, "Try again")}
            </Button>
          ),
        }
      : look === "payg" && entry.provider === "anthropic"
        ? { title: lim(lang, "Pay as you go, billed to your Anthropic account") }
        : null;
  return (
    <PlanCard
      testId="plan-card"
      plan={
        look === "payg" ? planTitle(lang, kind, look) : (entry.plan ?? w.t("{name} plan", { name }))
      }
      updated={
        figures && entry.checked_at !== undefined
          ? lim(lang, "As of {time}", { time: stamp(lang, entry.checked_at, now) })
          : null
      }
      stale={look === "stale"}
      windows={figures ? entry.windows.map((row) => windowOf(lang, row, now)) : []}
      message={message}
      foot={null}
    />
  );
}
