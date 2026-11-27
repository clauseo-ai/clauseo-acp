// The other plan held for the next message (issue 4a; design, "The status line, designed once"): in a conversation the
// status line's plan is a menu of both plans. Picking the other one changes nothing yet and opens no dialog: the line
// says "ChatGPT from your next message", shows that plan's own model and effort (each changeable there, kept with the
// hold, each plan keeping its own picks), and offers "Back to Claude", which drops the hold, as picking Claude does.
// The next message sent goes with the held agent as SendPrompt.agent and the hold's picks as its config; then the hold
// ends, and the header names the new agent once the host has switched. A message never goes without the line naming
// its agent, model and effort.
import { useMemo, useState } from "react";
import type { ConfigMenu, PlanChoice, StatusBack } from "@clauseo/components/clauseo/composer.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import type {
  AgentId,
  AgentKind,
  AgentSummary,
  ConfigPicks,
  MatterSummary,
} from "../transport/protocol.ts";
import { kindOf, planName } from "./agent.ts";
import { loadingMenus, menusOf } from "./config-menus.ts";
import { configOf, draftShown } from "./pickers.ts";

/** What the next message carries while a plan is held, whatever its delivery: the agent, and the model and effort the
 * held line shows (both keys on a change of model). */
export type HeldSend = { agent: AgentId; config?: ConfigPicks };

export type Hold = {
  /** The plan menu: both plans, the held one ticked; undefined with one plan or no thread. */
  choice: PlanChoice | undefined;
  /** The plan words while held, "ChatGPT from your next message"; null when nothing is held. */
  plan: string | null;
  /** The held plan's model and effort; undefined when nothing is held. */
  menus: readonly ConfigMenu[] | undefined;
  /** "Back to Claude"; null when nothing is held. */
  back: StatusBack | null;
  /** What the next send carries; null when nothing is held. */
  send: HeldSend | null;
  /** Ends the hold, as a sent message does. */
  clear: () => void;
};

/** The hold for the open conversation, whose agent is `agent` (null before it has a thread). Work is keyed by
 * conversation, so a hold never follows the lawyer to another one. */
export function useHold({
  agent,
  agents,
  options,
  matter,
  lang,
  sandboxRequired = false,
}: {
  agent: AgentId | null;
  agents: readonly AgentSummary[];
  /** Both plans, as the draft's choice lists them (conversations-view.ts's `options`). */
  options: readonly { value: AgentId; label: string }[];
  matter: MatterSummary | null;
  lang: Lang;
  /** The firm requires the sandbox: no probe of the agents' models runs (config-menus.ts's missingModels). */
  sandboxRequired?: boolean;
}): Hold {
  const [held, setHeld] = useState<AgentId | null>(null);
  // each plan's own picks while held, kept when the lawyer goes back and holds it again
  const [picks, setPicks] = useState<Partial<Record<AgentKind, ConfigPicks>>>({});
  const prefs = matter?.agent_prefs;
  return useMemo(() => {
    const clear = () => setHeld(null);
    if (agent === null) {
      return { choice: undefined, plan: null, menus: undefined, back: null, send: null, clear };
    }
    const own = kindOf(agent, agents);
    const kind = held === null ? undefined : kindOf(held, agents);
    const holding = held !== null && kind !== own;
    const ownValue = options.find((o) => kindOf(o.value, agents) === own)?.value ?? agent;
    const choice: PlanChoice | undefined =
      options.length > 1
        ? {
            value: holding ? held : ownValue,
            options,
            onValueChange: (next: AgentId) => setHeld(kindOf(next, agents) === own ? null : next),
          }
        : undefined;
    if (!holding) return { choice, plan: null, menus: undefined, back: null, send: null, clear };
    const summary = agents.find((a) => a.id === held);
    const models = summary?.models;
    const mine = kind === undefined ? undefined : picks[kind];
    const remembered = kind === undefined ? undefined : prefs?.[kind];
    const shown = draftShown(models, mine, remembered);
    const scope = `held:${held}`;
    const menus =
      shown === undefined
        ? loadingMenus(lang, scope, summary, sandboxRequired)
        : menusOf(
            lang,
            shown,
            (chosen) => {
              if (kind !== undefined)
                setPicks((now) => ({ ...now, [kind]: { ...now[kind], ...chosen } }));
            },
            scope,
          );
    // the held line's model and effort, picked or not: every send names what it goes with
    const config = shown === undefined ? undefined : configOf(shown.models, shown);
    return {
      choice,
      plan: th(lang, "{agent} from your next message", { agent: planName(held, agents) }),
      menus,
      back: {
        label: th(lang, "Back to {agent}", { agent: planName(agent, agents) }),
        onBack: clear,
      },
      send: { agent: held, ...(config === undefined ? {} : { config }) },
      clear,
    };
  }, [agent, agents, options, held, picks, prefs, lang, sandboxRequired]);
}
