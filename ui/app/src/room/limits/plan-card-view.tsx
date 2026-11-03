// The plan's card, opened from the plan in the toolbar (U12): "As of {time}", then every window with its share left,
// its bar and its reset, from the host's limits view. A window whose reset passed (the host reads again) shows no bar
// and no share, only when it reset, muted. With no figures the card says so and offers Try again (`refresh_limits`,
// `try_again`); billed by use, it has its title and who bills, no bar and no figure. Its own chunk: only a lawyer who
// opens the card needs it. Not here until the host gives them (H16, H11): the stale line's retry time, the month's
// figure, what happens at the limit, Manage plan.
import { PlanCard } from "@clauseo/components/clauseo/plan-card.tsx";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { pc } from "@clauseo/components/lib/words-plan-card.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useAgentLimits, useLimitsStore, useNow } from "../../store/limits.ts";
import type { AgentKind } from "../../transport/protocol.ts";
import { stamp } from "../thread/clock.ts";
import { PLAN_NAME, lookOf, planTitle } from "./plan-model.ts";
import { windowOf } from "./plan-windows.ts";

export function LivePlanCard({ kind }: { kind: AgentKind }) {
  const w = useWords();
  const entry = useAgentLimits(kind);
  const store = useLimitsStore();
  // times against this render's clock: the host sends a new view at each shown reset, nothing ticks here
  const now = useNow();
  if (entry === undefined) return null;
  const { lang } = w;
  const look = lookOf(entry);
  const figures = look === "figures" || look === "stale";
  const name = PLAN_NAME[kind];
  const message =
    look === "none"
      ? {
          title: pc(lang, "Usage isn't available right now."),
          body: pc(
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
        ? { title: pc(lang, "Pay as you go, billed to your Anthropic account") }
        : null;
  return (
    <PlanCard
      testId="plan-card"
      plan={
        look === "payg" ? planTitle(lang, kind, look) : (entry.plan ?? w.t("{name} plan", { name }))
      }
      updated={
        figures && entry.checked_at !== undefined
          ? pc(lang, "As of {time}", { time: stamp(lang, entry.checked_at, now) })
          : null
      }
      stale={look === "stale"}
      windows={figures ? entry.windows.map((row) => windowOf(lang, row, now)) : []}
      message={message}
      foot={null}
    />
  );
}
