// The plan in the toolbar, live (U12): the open conversation's agent's plan from the limits store, its line by the
// share used, and the card it opens: its own chunk, loaded as the pointer comes over the plan or focus reaches it, or at
// the latest on the first press, its head naming the plan while it lands; opening it asks the host for fresh figures
// (`refresh_limits`, `shown`). An agent of no kind (fake-agent) has no plan, and no line stands before the
// first view names the agent's kind. A part of its own, so a new view renders the plan alone, not the room.
import { lazy, Suspense } from "react";
import { ToolbarPlan } from "@clauseo/components/clauseo/room-toolbar.tsx";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useAgentLimits, useLimitsStore, useNow } from "../../store/limits.ts";
import type { AgentId } from "../../transport/protocol.ts";
import { kindOf } from "../agent.ts";
import type { AgentSummary } from "../host-next.ts";
import { preloadPlanCard } from "./plan-card-load.ts";
import { planLine } from "./plan-model.ts";

const LivePlanCard = lazy(() => preloadPlanCard().then((m) => ({ default: m.LivePlanCard })));

export function LivePlan({
  agent,
  agents,
}: {
  /** The open conversation's agent; null while the room doesn't know it. */
  agent: AgentId | null;
  /** The agents the host lists, for their kinds. */
  agents: readonly AgentSummary[];
}) {
  const w = useWords();
  const kind = agent === null ? undefined : kindOf(agent, agents);
  const entry = useAgentLimits(kind);
  const store = useLimitsStore();
  const now = useNow();
  // a hidden marker in every state, for the end-to-end specs to wait on: the entry's freshness ("live_only" while the
  // line is hidden), "none" before a view names the agent or for an agent of no kind; nothing shows, nothing is said
  const marker = (
    <span data-testid="plan-state" data-freshness={entry?.freshness ?? "none"} hidden />
  );
  if (kind === undefined || entry === undefined) return agent === null ? null : marker;
  // times against this render's clock: the host sends a new view at each shown reset, nothing ticks here
  const line = planLine(w.lang, kind, entry, now);
  if (line === null) return marker;
  return (
    <>
      {marker}
      <ToolbarPlan
        plan={{
          ...line,
          testId: "plan",
          // the first read still running: the name alone, with nothing to open
          ...(line.look === "loading"
            ? {}
            : {
                card: (
                  // while the chunk lands, the card's head with the plan's name, as the card names it
                  <Suspense
                    fallback={
                      <div className="plan-card-body">
                        <div className="plan-card-head">
                          <p className="plan-card-name">
                            {line.look === "bare"
                              ? line.name
                              : (entry.plan ?? w.t("{name} plan", { name: line.name }))}
                          </p>
                        </div>
                      </div>
                    }
                  >
                    <LivePlanCard kind={kind} />
                  </Suspense>
                ),
                cardLabel: lim(w.lang, "Show the plan's limits"),
                onCardOpen: () => store?.refresh(kind, "shown"),
                onCardWarm: () => void preloadPlanCard(),
              }),
        }}
      />
    </>
  );
}
