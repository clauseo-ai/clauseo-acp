// The other plan held for the next message (issue 4a; design, "The status line, designed once"): in a conversation the
// status line's plan is a menu of both plans. Picking the other one changes nothing yet and opens no dialog: the line
// says "ChatGPT from your next message", shows that plan's own model and effort (each changeable there, kept with the
// hold, each plan keeping its own picks), and offers "Back to Claude", which drops the hold, as picking Claude does.
// The next message sent goes with the held agent as SendPrompt.agent and the hold's picks as its config. The hold stays
// until the header names the new agent (a switch queued behind a running answer starts only after it, and every
// message in between goes to the new agent too, with its picks), or until the switch's turn fails as too long to hand
// over (work.tsx). A message never goes without the line naming its agent, model and effort.
// The hold lives in the room's `holds`, by thread, as the drafts do (room.tsx keeps both above Work's key), so a
// conversation left and opened again while its switch waits in the queue still shows and sends the held plan. The
// host's view can't say it: a queued UserMessageEntry has no agent or config. Once the switching message is gone
// from the queue without the header naming its agent (removed, or its turn over and failed), the hold ends (work.tsx).
// The room's header names the agent from the send (useShownAgent): once the host took the message with the hold (queued
// or started), that agent and its plan, though the host's header names the new agent only when the turn starts. The
// hold itself still ends on the host's header alone, so a queued switch keeps sending with the held plan.
import { useCallback, useEffect, useMemo, useState } from "react";
import type { ConfigMenu, PlanChoice, StatusBack } from "@clauseo/components/clauseo/composer.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import type {
  AgentId,
  AgentKind,
  AgentSummary,
  ConfigPicks,
  MatterSummary,
  EntryId,
  StreamId,
  ThreadId,
  TurnId,
} from "../transport/protocol.ts";
import { useStreamPart } from "../store/hooks.ts";
import type { ViewStore } from "../store/store.ts";
import { entryId, streamOf, type StreamView } from "../store/view.ts";
import { kindOf, planName } from "./agent.ts";
import { loadingMenus, menusOf } from "./config-menus.ts";
import { configOf, draftShown } from "./pickers.ts";

/** What the next message carries while a plan is held, whatever its delivery: the agent, and the model and effort the
 * held line shows (both keys on a change of model). */
export type HeldSend = { agent: AgentId; config?: ConfigPicks };

/** A thread's hold as the room keeps it: the held agent, each plan's own picks, and the turn of the message sent with
 * it once the host took one. */
export type Kept = {
  held: AgentId;
  picks: Partial<Record<AgentKind, ConfigPicks>>;
  turn?: TurnId;
};

/** The room's holds, by thread (room.tsx). */
export type Holds = Map<ThreadId, Kept>;

/** Whether the message sent with the hold, on `turn`, can no longer make the switch: removed from the queue, or its
 * turn over (its step or limit stop in the view) with the header still naming the old agent (its turn failed). A turn
 * that started still counts: the host's header names the new agent only once the turn is under way, and until then the
 * room's header names it from the send (useShownAgent). Not yet in the view counts as still waiting. */
export function sentGone(v: StreamView | undefined, turn: TurnId): boolean {
  if (v === undefined) return false;
  for (const entry of v.entries.values()) {
    const m = entry.user_message;
    if (m === undefined || m.turn !== turn) continue;
    if (m.status === "removed") return true;
    if (m.status === "queued") return false;
    return v.entries.has(entryId("step", turn)) || v.entries.has(entryId("limit_stop", turn));
  }
  return false;
}

type State = {
  thread: ThreadId | null;
  held: AgentId | null;
  picks: Partial<Record<AgentKind, ConfigPicks>>;
  turn: TurnId | null;
};

function keptState(thread: ThreadId | null, holds: Holds | undefined): State {
  const kept = thread === null ? undefined : holds?.get(thread);
  return { thread, held: kept?.held ?? null, picks: kept?.picks ?? {}, turn: kept?.turn ?? null };
}

/** The hold's state for `thread`, from and back to the room's `holds`; it ends once the header's `agent` is the held
 * one. */
function useKept(
  thread: ThreadId | null,
  holds: Holds | undefined,
  agent: AgentId | null,
  agents: readonly AgentSummary[],
) {
  // each plan's own picks while held, kept when the lawyer goes back and holds it again
  const [state, setState] = useState<State>(() => keptState(thread, holds));
  const { held, picks } = state;
  // set while rendering, as React allows for state that follows a prop: the thread came (or changed), so its kept hold;
  // or the header names the held agent: the switch happened, and the hold has done its work
  if (state.thread !== thread) setState(keptState(thread, holds));
  else if (held !== null && agent !== null && kindOf(held, agents) === kindOf(agent, agents))
    setState((s) => ({ ...s, held: null, turn: null }));
  useEffect(() => {
    if (thread === null || holds === undefined || state.thread !== thread) return;
    if (state.held === null) holds.delete(thread);
    else
      holds.set(thread, {
        held: state.held,
        picks: state.picks,
        ...(state.turn === null ? {} : { turn: state.turn }),
      });
  }, [holds, thread, state]);
  // one function each for the hold's life, so an effect that ends it doesn't run again on every change of the hold
  const clear = useCallback(() => setState((s) => ({ ...s, held: null, turn: null })), []);
  const sent = useCallback((turn: TurnId) => setState((s) => ({ ...s, turn })), []);
  const setHeld = useCallback(
    (next: AgentId | null) => setState((s) => ({ ...s, held: next, turn: null })),
    [],
  );
  const setPicks = useCallback(
    (
      f: (now: Partial<Record<AgentKind, ConfigPicks>>) => Partial<Record<AgentKind, ConfigPicks>>,
    ) => setState((s) => ({ ...s, picks: f(s.picks) })),
    [],
  );
  const turn = state.turn;
  return { held, picks, turn, clear, sent, setHeld, setPicks };
}

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
  /** Ends the hold: Back, or a switch that couldn't happen. */
  clear: () => void;
  /** The turn of the message the host took with the hold; null before one. */
  turn: TurnId | null;
  /** The host took a message sent with the hold, on `turn`. */
  sent: (turn: TurnId) => void;
};

/** The hold for the open conversation `thread`, whose agent is `agent` (null before it has a thread), kept in the
 * room's `holds` by thread, so a hold never follows the lawyer to another conversation and is there on coming back. */
export function useHold({
  thread = null,
  holds,
  agent,
  agents,
  options,
  matter,
  lang,
  sandboxRequired = false,
}: {
  thread?: ThreadId | null;
  holds?: Holds | undefined;
  agent: AgentId | null;
  agents: readonly AgentSummary[];
  /** Both plans, as the draft's choice lists them (conversations-view.ts's `options`). */
  options: readonly { value: AgentId; label: string }[];
  matter: MatterSummary | null;
  lang: Lang;
  /** The firm requires the sandbox: no probe of the agents' models runs (config-menus.ts's missingModels). */
  sandboxRequired?: boolean;
}): Hold {
  const prefs = matter?.agent_prefs;
  const { held, picks, turn, clear, sent, setHeld, setPicks } = useKept(
    thread,
    holds,
    agent,
    agents,
  );
  return useMemo(() => {
    if (agent === null) {
      return {
        choice: undefined,
        plan: null,
        menus: undefined,
        back: null,
        send: null,
        clear,
        turn,
        sent,
      };
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
    if (!holding)
      return { choice, plan: null, menus: undefined, back: null, send: null, clear, turn, sent };
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
      turn,
      sent,
    };
  }, [
    agent,
    agents,
    options,
    held,
    picks,
    prefs,
    lang,
    sandboxRequired,
    clear,
    sent,
    setHeld,
    setPicks,
    turn,
  ]);
}

/** Ends the hold when its switch can't happen any more: the switch's turn failed as too long to hand over
 * (`tooLong`, that break's id, null for none), or the message sent with it left the queue without the header naming
 * its agent (removed, or failed; sentGone). */
export function useHoldEnds(
  hold: Hold,
  store: ViewStore,
  stream: StreamId | null,
  tooLong: EntryId | null,
): void {
  const { clear, turn } = hold;
  const gone = useStreamPart(
    store,
    stream,
    useCallback((v: StreamView | undefined) => turn !== null && sentGone(v, turn), [turn]),
  );
  useEffect(() => {
    if (tooLong !== null || gone) clear();
  }, [tooLong, gone, clear]);
}

const headerAgentPick = (v: StreamView | undefined) => v?.header?.header?.agent ?? null;

/** The agent the room names for the open conversation `thread` (null for a draft or none): the held agent once the host
 * took a message sent with the hold (Work's onSwitch), else the agent the thread's header names, else `listed`, the
 * conversations' row's (which follows the host's list, behind the header). `headerAgent` is the header's own, for
 * its agent_name: a name of the agent before the switch is not the new one's. */
export function useShownAgent(
  store: ViewStore,
  thread: ThreadId | null,
  listed: AgentId,
): {
  agent: AgentId;
  headerAgent: AgentId | null;
  onSwitch: (thread: ThreadId, agent: AgentId | null) => void;
} {
  const [switched, setSwitched] = useState<{ thread: ThreadId; agent: AgentId } | null>(null);
  const onSwitch = useCallback(
    (from: ThreadId, agent: AgentId | null) =>
      setSwitched((s) =>
        agent === null
          ? s?.thread === from
            ? null
            : s
          : s?.thread === from && s.agent === agent
            ? s
            : { thread: from, agent },
      ),
    [],
  );
  const headerAgent = useStreamPart(
    store,
    thread === null ? null : streamOf(thread),
    headerAgentPick,
  );
  const agent =
    (thread !== null && switched?.thread === thread ? switched.agent : null) ??
    headerAgent ??
    listed;
  return { agent, headerAgent, onSwitch };
}
