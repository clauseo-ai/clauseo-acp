// The pickers' copy review entries (PK-01 to PK-05): the agent's model and effort on the composer's status line, and
// the plan not set up on this computer, each in the real room over a FakeTransport with the setup wire, the agents'
// models as the host's probe filled them (pick-models.ts), and the scene's steps taken in order (RoomDriven). The
// model and value names are the agents' own, never Clauseo's words, so they are fake here. Apart from registry.tsx to
// keep that file under the size limit; registry-uc-composer-menus.tsx renders its scenes too.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { sx, type SigninExpiredKey } from "@clauseo/components/lib/words-signin-expired.ts";
import { batch, event, header, reset } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  AgentId,
  AgentModels,
  ConfigOption,
  HeaderEntry,
  NamedChoice,
  ProtectionView,
  SettingsView,
  SetupEvent,
  SignIn,
  ThreadId,
  ThreadSummary,
  ViewEntry,
} from "../transport/protocol.ts";
import { detailed, fakeNext } from "../room/fake-next.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { CLAUDE_MODELS, CODEX_MODELS } from "./pick-models.ts";
import { RoomDriven, type DrivenStep } from "./registry-ud1-switching.tsx";
import { firstLaunchWords, threadWords, type WordsRef } from "./words.ts";

export const CLAUDE: AgentId = "claude-acp";
export const CODEX: AgentId = "codex-acp";
const AT = new Date(2026, 10, 10, 10, 12).getTime();
const MADE: ThreadId = "00000000-0000-7000-8000-0000000000c9";

/** The session's options as a conversation's header holds them, from the agent's models: the model at its current
 * value, the current model's efforts, and a mode the pickers never offer. */
export function optionsOf(models: AgentModels): ConfigOption[] {
  const current = models.models.find((m) => m.id === models.current_model);
  const select = (id: string, category: string, value: string, choices: NamedChoice[]) =>
    ({
      id,
      name: id,
      category,
      kind: {
        select: {
          current_value: value,
          options: choices.map((c) => ({ value: c.id, name: c.name })),
        },
      },
    }) as ConfigOption;
  const efforts = current?.efforts ?? [];
  return [
    select("model", "model", models.current_model, models.models),
    ...(models.effort_option === undefined || efforts.length === 0
      ? []
      : [select(models.effort_option, "thought_level", models.current_effort ?? "", efforts)]),
    select("mode", "mode", "default", [{ id: "default", name: "Default" }]),
  ];
}

/** Each agent's models, as the host's probe filled them. */
export const MODELS: Record<AgentId, AgentModels> = {
  [CLAUDE]: CLAUDE_MODELS,
  [CODEX]: CODEX_MODELS,
};

/** This computer protected at tier a, the firm requiring the sandbox. */
const REQUIRED: ProtectionView = {
  tier: "a",
  computer: { protected: { tier: "a" } },
  required: true,
  unprotected_auto_allowed: true,
};

export const install = (agent: AgentId): SetupEvent => ({
  agent_install_progress: { agent, state: { ready: { version: "0.1.0" } } },
});
export const signIn = (agent: AgentId, state: SignIn): SetupEvent => ({
  sign_in_changed: { agent, state },
});
export const SIGNED: SignIn = {
  signed_in: { email: "a.mehra@example.com", method: "plan", source: "own", at: AT },
};
export const OUT: SignIn = { signed_out: {} };
export const EXPIRED: SignIn = { failed: { code: "agent.auth_required", at: AT } };

export type PickScene = {
  /** The setup events the room hears first. */
  events: SetupEvent[];
  /** The matter's conversations, newest last; none opens a new conversation. */
  threads?: { thread: ThreadId; agent: AgentId; title: string }[];
  /** The matter's remembered picks, by plan. */
  prefs?: { claude?: Record<string, string>; codex?: Record<string, string> };
  /** The agents listed (R15), each with its options; both by default. */
  agents?: AgentId[];
  steps?: DrivenStep[];
  /** Each agent's models; MODELS by default. An agent left out, or null, has none yet (the probe hasn't run). */
  models?: Record<AgentId, AgentModels | null>;
  /** The open thread's header, over the scene's: its status, what its turn ran with, code mode. */
  head?: Partial<HeaderEntry>;
  /** Whether the open thread's session reported its options; true by default. */
  live?: boolean;
  /** The open thread's entries; none by default. */
  entries?: ViewEntry[];
  /** The agents whose models the host's probe couldn't read (AgentSummary.models_unread). */
  unread?: AgentId[];
  /** The firm requires the sandbox (ProtectionView.required), this computer protected. */
  required?: boolean;
};

const SETTINGS = {
  ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
  privacy: { counts: true, crash_reports: false, counts_started: true },
  locked: [],
} as unknown as SettingsView;

/** The room over a fake host with the setup wire, the agents' options, and each thread's header as it subscribes. */
export function pickHost(s: PickScene): () => FakeTransport {
  const matter = { ...MATTER, ...(s.prefs === undefined ? {} : { agent_prefs: s.prefs }) };
  const threads: ThreadSummary[] = (s.threads ?? []).map((t, i) => ({
    thread: t.thread,
    matter: MATTER.matter,
    agent: t.agent,
    status: "idle",
    title: t.title,
    last_activity: AT - (s.threads!.length - i) * 3_600_000,
  }));
  return () => {
    const transport: FakeTransport = new FakeTransport({
      platform: "mac",
      openSettings: () => {},
      query: (query) => {
        if (query === "list_matters") return { matters: [matter] };
        if (query === "get_settings") return { settings: SETTINGS };
        if (typeof query === "object" && "list_threads" in query) return { threads };
        throw new Error(`not scripted: ${JSON.stringify(query)}`);
      },
      dispatch: (envelope) => {
        const c = envelope.command;
        if (c.create_thread !== undefined) return { thread_created: { thread: MADE } };
        return "done";
      },
      setup: { first: () => [{ setup_info: { keys_available: true } }, ...s.events] },
      ...(s.required === true ? { protection: { view: () => REQUIRED } } : {}),
      next: fakeNext({
        matters: () => [detailed(matter)],
        agents: () =>
          (s.agents ?? [CLAUDE, CODEX]).map((id) => {
            const models = (s.models ?? MODELS)[id] ?? undefined;
            return {
              id,
              kind: id === CLAUDE ? ("claude" as const) : ("codex" as const),
              ...(models === undefined ? {} : { models, config_options: optionsOf(models) }),
              ...(s.unread?.includes(id) === true ? { models_unread: true } : {}),
            };
          }),
        parts: ["agents"],
      }),
      subscribe: (stream) =>
        queueMicrotask(() => {
          const open = threads.at(-1);
          if (open === undefined) return;
          const head = header();
          head.header!.thread = open.thread;
          head.header!.matter = MATTER.matter;
          head.header!.agent = open.agent;
          const models = (s.models ?? MODELS)[open.agent] ?? undefined;
          head.header!.config_options =
            models === undefined || s.live === false ? [] : optionsOf(models);
          Object.assign(head.header!, s.head);
          transport.deliver(event(batch(1, [reset(s.entries ?? [], head)], undefined, stream)));
        }),
    });
    return transport;
  };
}

export const renderPick = (s: PickScene) => () => (
  <RoomDriven host={pickHost(s)} steps={s.steps ?? []} />
);

const expiredWords = (key: SigninExpiredKey): WordsRef => ({
  file: "lib/words-signin-expired.ts",
  key,
  text: (lang: Lang) => sx(lang, key),
  later: sx.hi[key] === null,
});

/** The status line's ChatGPT side of the plan switch, Claude first. */
export const CHATGPT = ".room-status-seg button:nth-of-type(2)";
export const BOTH = [
  install(CLAUDE),
  install(CODEX),
  signIn(CLAUDE, SIGNED),
  signIn(CODEX, SIGNED),
];
export const CLAUDE_ONLY = [install(CLAUDE), signIn(CLAUDE, SIGNED), signIn(CODEX, OUT)];
const T1 = "00000000-0000-7000-8000-0000000000c1";
const T2 = "00000000-0000-7000-8000-0000000000c2";
export const ONGOING = [
  { thread: T1, agent: CLAUDE, title: "Termination for convenience" },
  { thread: T2, agent: CLAUDE, title: "Notice under clause 14" },
];

const screen = "room/composer-pickers";

export const PK: Entry[] = [
  // a new conversation, both plans set up: the model menu open over the status line
  {
    id: "PK-01",
    words: threadWords("Model"),
    screen,
    render: renderPick({ events: BOTH, steps: [{ press: '[data-testid="pick-model"]' }] }),
  },
  // an ongoing conversation, its plan fixed: the effort menu open
  {
    id: "PK-02",
    words: threadWords("Effort"),
    screen,
    render: renderPick({
      events: BOTH,
      threads: ONGOING,
      steps: [{ press: '[data-testid="pick-effort"]' }],
    }),
  },
  // UC-152 ("(default)"), UC-874 ("{agent}'s default") and UC-884 ("Default effort"), retired: the host sends real
  // models with no seed and no Default row (stage B's B2), and the menus name each model as the agent does. Never
  // reuse these ids
  // a new conversation, ChatGPT chosen but not set up on this computer
  {
    id: "PK-03",
    words: firstLaunchWords("{name} isn't set up on this computer."),
    screen,
    render: renderPick({ events: CLAUDE_ONLY, agents: [CLAUDE], steps: [{ press: CHATGPT }] }),
  },
  // a conversation fixed to ChatGPT, ChatGPT gone from this computer
  {
    id: "PK-04",
    words: firstLaunchWords("Set up {name}"),
    screen,
    render: renderPick({
      events: CLAUDE_ONLY,
      agents: [CLAUDE],
      threads: [{ thread: T1, agent: CODEX, title: "Termination for convenience" }],
    }),
  },
  // a new conversation, ChatGPT chosen, its sign-in expired
  {
    id: "PK-05",
    words: expiredWords("Your {name} sign-in has expired."),
    screen,
    render: renderPick({
      events: [install(CLAUDE), install(CODEX), signIn(CLAUDE, SIGNED), signIn(CODEX, EXPIRED)],
      steps: [{ press: CHATGPT }],
    }),
  },
];
