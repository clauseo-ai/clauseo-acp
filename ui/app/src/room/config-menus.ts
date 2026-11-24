// The model and effort menus on the composer's status line (pickers.ts's rules, status-line.tsx's ConfigMenu): a new
// conversation's from its plan's models (AgentSummary.models), its values the draft's picks over the matter's
// remembered ones over what a new session starts on, each pick kept in the draft (conversations.ts) for create_thread;
// a conversation's from its header's options read with its agent's models. A conversation's line names the turn's
// record (the header's model and effort), during and after an answer; the lawyer's pick stays in the composer, ticked
// in the menu and named as "Next message: …", and goes with every send as SendPrompt.config (a change of model with
// the effort beside it), steers included: the host drops it from a steer that joins the running answer. It stands
// until a turn starts whose record matches it, or a pick_not_used notice comes after it. Until the agent's models are
// in, one quiet line stands in the menus' place, never a made-up default.
import { useCallback, useMemo, useState } from "react";
import type { ConfigMenu } from "@clauseo/components/clauseo/status-line.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useStreamPart } from "../store/hooks.ts";
import type { ViewStore } from "../store/store.ts";
import type { StreamView } from "../store/view.ts";
import type {
  AgentSummary,
  ConfigOption,
  ConfigPicks,
  MatterSummary,
  NamedChoice,
  StreamId,
  ThreadId,
} from "../transport/protocol.ts";
import { kindOf } from "./agent.ts";
import { turnRunning } from "./thread/live.ts";
import type { Conversations, ConversationsView } from "./conversations.ts";
import {
  configOf,
  draftShown,
  effortsOf,
  modelPick,
  nextPicks,
  pickStands,
  threadShown,
  type NextPick,
  type Shown,
} from "./pickers.ts";

const NO_OPTIONS: readonly ConfigOption[] = [];
const noop = () => {};

/** What stands in the menus' place while the agent's models aren't in, one line of words:
 * - the host's probe failed or timed out and nothing fills them (AgentSummary.models_unread, true through its bounded
 *   retries): that the list couldn't be read, and that asking still works;
 * - the firm requires the sandbox (sandbox.required): no probe runs, and the models come with the first session;
 * - else the probe is on its way: "Loading models…".
 * An agent not signed in never gets here: its sign-in takes the composer, menus and all. Ask works in every case. */
export function missingModels(
  lang: Lang,
  agent: AgentSummary | undefined,
  sandboxRequired: boolean,
): string {
  if (agent?.models_unread === true)
    return th(lang, "Couldn't read the models yet. You can still ask.");
  if (sandboxRequired) return th(lang, "Models show after your first message.");
  return th(lang, "Loading models…");
}

/** The line in the menus' place while the agent's models aren't in: a menu with no values, drawn as its words. */
export const loadingMenus = (
  lang: Lang,
  scope: string | undefined,
  agent: AgentSummary | undefined,
  sandboxRequired: boolean,
): readonly ConfigMenu[] => [
  {
    id: "loading",
    scope,
    label: missingModels(lang, agent, sandboxRequired),
    value: "",
    options: [],
    onValueChange: noop,
  },
];

/** The menus for what `shown` shows, in `scope`: the model, each row its name and, when the agent gives one, its
 * description; then the effort of the model shown, left out for a model that takes none. A change of model goes to
 * `pick` with the effort the new model takes (modelPick); an effort alone, by itself. */
export function menusOf(
  lang: Lang,
  shown: Shown,
  pick: (picks: ConfigPicks) => void,
  scope?: string,
  /** A conversation's pick for the next message, ticked in the menus while the triggers name the record. */
  next?: { model: string; effort: string },
): readonly ConfigMenu[] {
  const { models } = shown;
  const nameOf = (id: string, list: readonly { id: string; name: string }[]) =>
    list.find((m) => m.id === id)?.name;
  const model: ConfigMenu = {
    id: models.model_option,
    scope,
    label: th(lang, "Model"),
    value: next?.model ?? shown.model,
    ...(next === undefined || next.model === shown.model
      ? {}
      : { atRest: nameOf(shown.model, models.models) ?? th(lang, "Model") }),
    options: models.models.map((m) => ({
      value: m.id,
      name: m.name,
      ...(m.description === undefined ? {} : { description: m.description }),
    })),
    onValueChange: (value) => pick(modelPick(shown, value)),
  };
  const ek = models.effort_option;
  // a conversation's record that names no model or no effort shows no menu for it, unless a pick names one
  const picked = next !== undefined && next.model !== shown.model;
  const head = shown.record === true && shown.model === "" && !picked ? [] : [model];
  // the effort menu lists the efforts of the model the next message goes with
  const efforts = picked ? effortsOf(models, next.model) : shown.efforts;
  const effortValue = next?.effort ?? shown.effort;
  if (
    ek === undefined ||
    efforts.length === 0 ||
    (shown.record === true && shown.effort === "" && next?.effort === undefined)
  )
    return head;
  const effort: ConfigMenu = {
    id: ek,
    scope,
    label: th(lang, "Effort"),
    value: effortValue,
    ...(effortValue === shown.effort
      ? {}
      : { atRest: nameOf(shown.effort, shown.efforts) ?? th(lang, "Effort") }),
    options: efforts.map((e) => ({ value: e.id, name: e.name })),
    onValueChange: (value) => pick({ [ek]: value }),
  };
  return [...head, effort];
}

/** A new conversation's menus, while the open conversation is a draft: undefined otherwise. */
export function useDraftMenus(
  talks: ConversationsView,
  conversations: Conversations,
  matter: MatterSummary | null,
  lang: Lang,
  /** The firm requires the sandbox: the host runs no probe, so the models come with the first session. */
  sandboxRequired: boolean,
): readonly ConfigMenu[] | undefined {
  const { drafting, agent, agents, picks } = talks;
  const prefs = matter?.agent_prefs;
  return useMemo(() => {
    if (!drafting) return undefined;
    const kind = kindOf(agent, agents);
    const summary = agents.find((a) => a.id === agent);
    const models = summary?.models;
    const picked = kind === undefined ? undefined : picks[kind];
    const remembered = kind === undefined ? undefined : prefs?.[kind];
    // a new conversation's, for its plan: another plan's values come in silently
    const scope = `draft:${agent}`;
    const shown = draftShown(models, picked, remembered);
    if (shown === undefined) return loadingMenus(lang, scope, summary, sandboxRequired);
    return menusOf(lang, shown, (chosen) => conversations.pick(chosen), scope);
  }, [drafting, agent, agents, picks, prefs, lang, conversations, sandboxRequired]);
}

/** What the menus read of a thread's header: its options, its agent, what its latest turn ran with, and whether a
 * turn runs. Any change drops a pick waiting on the host: a turn starting or ending, the agent's answer. */
type HeaderPart = {
  options: readonly ConfigOption[];
  agent: string | null;
  model?: NamedChoice | undefined;
  effort?: NamedChoice | undefined;
  running: boolean;
};

const NO_PART: HeaderPart = { options: NO_OPTIONS, agent: null, running: false };

/** The header's part, the same object for as long as it reads the same: a header that changes for its status or its
 * usage, as it does through a turn, changes nothing here. */
function headerPick(): (v: StreamView | undefined) => HeaderPart {
  let last = NO_PART;
  let said = JSON.stringify(NO_PART);
  return (v) => {
    const h = v?.header?.header;
    if (h === undefined || h === null) return last === NO_PART ? last : (last = NO_PART);
    const next = {
      options: h.config_options ?? NO_OPTIONS,
      agent: h.agent,
      model: h.model,
      effort: h.effort,
      running: turnRunning(h.status),
    };
    const now = JSON.stringify(next);
    if (now !== said) [last, said] = [next, now];
    return last;
  };
}

/** The order of the latest pick_not_used notice in the view: a pick made before it has been answered. */
function noticesPick(): (v: StreamView | undefined) => number {
  // read again only when an entry comes or goes (the ids array is the same until then): a notice is never edited
  let ids: unknown = undefined;
  let last = 0;
  return (v) => {
    if (v === undefined || v.ids === ids) return last;
    ids = v.ids;
    let latest = 0;
    for (const e of v.entries.values()) {
      const kind = e.notice?.kind;
      if (typeof kind === "object" && kind !== null && "pick_not_used" in kind)
        latest = Math.max(latest, e.notice!.order);
    }
    last = latest;
    return last;
  };
}

/** A conversation's model and effort: its menus, and what the next message goes with (SendPrompt.config). */
export type ThreadMenus = {
  menus: readonly ConfigMenu[] | undefined;
  config: ConfigPicks | undefined;
};

const NO_THREAD: ThreadMenus = { menus: undefined, config: undefined };

/** A conversation's menus, from its header's options and its agent's models, and the next message's picks; no menus
 * with no thread. */
export function useThreadMenus(
  store: ViewStore,
  stream: StreamId | null,
  thread: ThreadId | null,
  agents: readonly AgentSummary[],
  lang: Lang,
  sandboxRequired = false,
): ThreadMenus {
  const [pickPart] = useState(headerPick);
  const part = useStreamPart(store, stream, pickPart);
  const [pickNotices] = useState(noticesPick);
  const notices = useStreamPart(store, stream, pickNotices);
  const summary = agents.find((a) => a.id === part.agent);
  const probed = summary?.models;
  // the lawyer's pick for the next message, with the latest pick_not_used notice it was made after
  const [pick, setPick] = useState<(NextPick & { notices: number }) | null>(null);
  const stands = pick !== null && pickStands(pick, part, notices) ? pick : null;
  const choose = useCallback(
    (picks: ConfigPicks, shown: Shown) => {
      const { model_option, effort_option } = shown.models;
      const model = picks[model_option];
      const effort = effort_option === undefined ? undefined : picks[effort_option];
      setPick({
        ...stands,
        ...(model === undefined ? {} : { model }),
        ...(effort === undefined ? {} : { effort }),
        notices,
      });
    },
    [stands, notices],
  );
  return useMemo(() => {
    if (thread === null) return NO_THREAD;
    const scope = `thread:${thread}`;
    const shown = threadShown(part.options, probed, part);
    if (shown === undefined)
      return { menus: loadingMenus(lang, scope, summary, sandboxRequired), config: undefined };
    const next = nextPicks(shown, stands);
    const menus = menusOf(lang, shown, (picks) => choose(picks, shown), scope, next);
    const config = configOf(shown.models, next);
    if (stands === null) return { menus, config };
    // the pick, named for the next message beside the record the triggers name
    const name = (id: string, list: readonly { id: string; name: string }[]) =>
      list.find((m) => m.id === id)?.name;
    const settings = [
      name(next.model, shown.models.models),
      name(next.effort, effortsOf(shown.models, next.model)),
    ].filter((n): n is string => n !== undefined);
    const mark: ConfigMenu = {
      id: "next",
      scope,
      label: th(lang, "Next message: {settings}", { settings: settings.join(" · ") }),
      value: "",
      options: [],
      onValueChange: noop,
    };
    return { menus: settings.length === 0 ? menus : [...menus, mark], config };
  }, [thread, part, probed, summary, stands, lang, choose, sandboxRequired]);
}
