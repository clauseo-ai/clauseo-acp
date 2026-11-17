// The model and effort menus on the composer's status line (pickers.ts's rules, status-line.tsx's ConfigMenu): a new
// conversation's from its plan's options as the agent last reported them, its values the draft's picks over the
// matter's remembered ones over the agent's own, each pick kept in the draft (conversations.ts) for create_thread; a
// conversation's from its header's options, each pick a SetConfigOption. A pick shows at once and stays shown until the
// header's options change (the agent's answer) or the host refuses it: then the menu follows the options as they are,
// with no error, since the host falls back to a value the agent offers and says so in the options.
import { useCallback, useMemo, useState } from "react";
import type { ConfigMenu } from "@clauseo/components/clauseo/status-line.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useStreamPart } from "../store/hooks.ts";
import type { ViewStore } from "../store/store.ts";
import type { StreamView } from "../store/view.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { ConfigOption, MatterSummary, StreamId, ThreadId } from "../transport/protocol.ts";
import { kindOf, planName } from "./agent.ts";
import type { Conversations, ConversationsView } from "./conversations.ts";
import {
  AGENT_DEFAULT_VALUE,
  DEFAULT_VALUE,
  draftPickables,
  draftShown,
  draftValue,
  pickables,
  resolvedDefault,
  setOption,
  type Pickable,
} from "./pickers.ts";

const NO_OPTIONS: readonly ConfigOption[] = [];

/** The menus for the options the pickers show, each value as `valueOf` says, each pick to `pick`, in `scope`. `plan`
 * names the agent's plan ("Claude", "ChatGPT") for the host's seed, before the agent has reported its options. */
export function menusOf(
  lang: Lang,
  shown: readonly Pickable[],
  valueOf: (p: Pickable) => string,
  pick: (p: Pickable, value: string) => void,
  scope?: string,
  plan?: string,
): readonly ConfigMenu[] {
  return shown.map((p) => {
    // the Default row in the name of what it stands for, so the trigger says "Opus 5.5", never "Default"; the menu
    // marks it quietly as the default. Unresolved, it keeps the agent's own name.
    const resolved = resolvedDefault(p);
    return {
      id: p.id,
      scope,
      label: th(lang, p.category === "model" ? "Model" : "Effort"),
      value: valueOf(p),
      options: p.choices.map((c) =>
        // the host's seed: one choice, the agent left to its own default; the model's named for the plan (UC-874), the
        // effort's in words of its own (UC-884), so the two triggers side by side never read the same
        c.value === AGENT_DEFAULT_VALUE && plan !== undefined
          ? {
              value: c.value,
              name:
                p.category === "model"
                  ? th(lang, "{agent}'s default", { agent: plan })
                  : th(lang, "Default effort"),
            }
          : c.value === DEFAULT_VALUE && resolved !== undefined
            ? { value: c.value, name: resolved, note: th(lang, "(default)") }
            : {
                value: c.value,
                name: c.name,
                ...(c.description === undefined ? {} : { description: c.description }),
              },
      ),
      onValueChange: (value) => pick(p, value),
    };
  });
}

/** A new conversation's menus, while the open conversation is a draft: undefined otherwise. Before a plan's agent has
 * reported its options (its first conversation reports them), the host's seed: one row a menu, "{agent}'s default" for the
 * model and "Default effort" for the effort, each left out when the matter remembers a pick for its option. */
export function useDraftMenus(
  talks: ConversationsView,
  conversations: Conversations,
  matter: MatterSummary | null,
  lang: Lang,
): readonly ConfigMenu[] | undefined {
  const { drafting, agent, agents, picks } = talks;
  const prefs = matter?.agent_prefs;
  return useMemo(() => {
    if (!drafting) return undefined;
    const kind = kindOf(agent, agents);
    const options = agents.find((a) => a.id === agent)?.config_options;
    const picked = kind === undefined ? undefined : picks[kind];
    const remembered = kind === undefined ? undefined : prefs?.[kind];
    return menusOf(
      lang,
      draftPickables(draftShown(options, picked, remembered), remembered),
      (p) => draftValue(p, picked?.[p.id], remembered?.[p.id]),
      (p, value) => conversations.pick(p.id, value),
      // a new conversation's, for its plan: another plan's values come in silently
      `draft:${agent}`,
      planName(agent, agents),
    );
  }, [drafting, agent, agents, picks, prefs, lang, conversations]);
}

/** The header's options, the same array for as long as they read the same: a header that changes for its status or
 * its usage, as it does through a turn, changes nothing here. */
function optionsPick(): (v: StreamView | undefined) => readonly ConfigOption[] {
  let last = NO_OPTIONS;
  let said = "[]";
  return (v) => {
    const next = v?.header?.header?.config_options ?? NO_OPTIONS;
    if (next === last) return last;
    const now = JSON.stringify(next);
    if (now !== said) [last, said] = [next, now];
    return last;
  };
}

/** A conversation's menus, from its header's options; undefined with no thread. */
export function useThreadMenus(
  transport: HostTransport,
  store: ViewStore,
  stream: StreamId | null,
  thread: ThreadId | null,
  lang: Lang,
): readonly ConfigMenu[] | undefined {
  const [pick] = useState(optionsPick);
  const options = useStreamPart(store, stream, pick);
  // the lawyer's pick, shown until the options it was made over change or the host refuses it
  const [pending, setPending] = useState<{
    over: readonly ConfigOption[];
    id: string;
    value: string;
  } | null>(null);
  const choose = useCallback(
    (p: Pickable, value: string) => {
      if (thread === null) return;
      const mine = { over: options, id: p.id, value };
      setPending(mine);
      transport
        .dispatch({ command_id: crypto.randomUUID(), command: setOption(thread, p.id, value) })
        .catch((error: unknown) => {
          // a value the agent no longer offers: the host keeps the agent's, and the menu shows it
          console.warn(`set_config_option refused: ${JSON.stringify(error)}`);
          setPending((now) => (now === mine ? null : now));
        });
    },
    [transport, thread, options],
  );
  return useMemo(() => {
    if (thread === null) return undefined;
    const held = pending !== null && pending.over === options ? pending : null;
    const valueOf = (p: Pickable) => (held !== null && held.id === p.id ? held.value : p.current);
    // the effort follows the model the lawyer just picked, shown at once
    return menusOf(lang, pickables(options, valueOf), valueOf, choose, `thread:${thread}`);
  }, [thread, options, pending, lang, choose]);
}
