// The plan in the toolbar, live (U12): the open conversation's agent's plan from the limits store, its line by the
// share used, and the card it opens: its own chunk, loaded as the pointer comes over the plan or focus reaches it, or at
// the latest on the first press, its head naming the plan while it lands; opening it asks the host for fresh figures
// (`refresh_limits`, `shown`). An agent of no kind (fake-agent) has no plan, and no line stands
// before the first view names the agent's kind. The plan's name and its line show only once the
// setup store says the agent is signed in (planNamedBy); until then, or signed out or signing in,
// the header names the agent alone. A part of its own, so a new view renders the plan alone,
// not the room.
import { lazy, Suspense, useState, useSyncExternalStore } from "react";
import { ToolbarPlan } from "@clauseo/components/clauseo/room-toolbar.tsx";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useAgentLimits, useLimitsStore, useNow } from "../../store/limits.ts";
import { CLAUDE, CODEX, setupStoreOf, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId, AgentKind, SignIn } from "../../transport/protocol.ts";
import { kindOf } from "../agent.ts";
import type { AgentSummary } from "../host-next.ts";
import { preloadPlanCard } from "./plan-card-load.ts";
import { nameOnlyLine, planLine, planNamedBy } from "./plan-model.ts";

const LivePlanCard = lazy(() => preloadPlanCard().then((m) => ({ default: m.LivePlanCard })));

/** The setup store's id for each kind's plan (the first run's CLAUDE and CODEX). */
const SETUP_ID: Record<AgentKind, AgentId> = { claude: CLAUDE, codex: CODEX };

/** The sign-in the setup store holds for a kind's plan, as the store has heard it; undefined
 * before its first event. */
function useSignIn(transport: HostTransport, kind: AgentKind | undefined): SignIn | undefined {
  const [store] = useState(() => setupStoreOf(transport as HostTransport & SetupSource));
  const get = () => (kind === undefined ? undefined : store.state().agents[SETUP_ID[kind]]?.signIn);
  return useSyncExternalStore(store.subscribe, get, get);
}

export function LivePlan({
  agent,
  agents,
  transport,
}: {
  /** The open conversation's agent; null while the room doesn't know it. */
  agent: AgentId | null;
  /** The agents the host lists, for their kinds. */
  agents: readonly AgentSummary[];
  /** The window's transport: its setup store says each plan's sign-in. */
  transport: HostTransport;
}) {
  const w = useWords();
  const kind = agent === null ? undefined : kindOf(agent, agents);
  const entry = useAgentLimits(kind);
  const store = useLimitsStore();
  const now = useNow();
  const signIn = useSignIn(transport, kind);
  // a hidden marker in every state, for the end-to-end specs to wait on: the entry's freshness ("live_only" while the
  // line is hidden), "none" before a view names the agent or for an agent of no kind; nothing shows, nothing is said
  const marker = (
    <span data-testid="plan-state" data-freshness={entry?.freshness ?? "none"} hidden />
  );
  if (kind === undefined || entry === undefined) return agent === null ? null : marker;
  // not signed in: the agent's name alone, once a view names the agent
  if (!planNamedBy(signIn))
    return (
      <>
        {marker}
        <ToolbarPlan plan={{ ...nameOnlyLine(kind, entry.freshness), testId: "plan" }} />
      </>
    );
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
