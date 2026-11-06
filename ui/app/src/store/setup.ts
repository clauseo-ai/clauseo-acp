// The first run's setup state in the window (UD1), in the first run's lazy chunk: each agent's install and sign-in from
// the host's `setup` stream (R20's SetupEvent), what this computer offers (SetupInfo), the anonymous counts switch
// before and after the lawyer's first act (S21), and the gate's own state (the key panel, the paste field, what was
// copied). Commands go back through the transport: `setup` (SetupCommand), `dispatch` for `first_act` and
// `set_privacy_settings`, and `count_ui` for the counts each screen sends, once each.
//
// Nothing here keeps a pasted code or an API key: they go from the gate's field straight into one `setup` command and
// are dropped. Nothing runs a timer: the state changes only when the host says something or the lawyer presses.
import type { HostTransport } from "../transport/host-transport.ts";
import type {
  AgentId,
  ErrorCode,
  InstallProgress,
  SettingsView,
  SetupCommand,
  SetupEvent,
  SignIn,
  UiCount,
  UiOnboardingStep,
} from "../transport/protocol.ts";

/** A transport that carries the setup wire: R20's `setup` command and stream, and S21's `count_ui`. The desktop one
 * carries desktop's own (tauri-transport.ts, setup-feed.ts); the fake scripts it. The room asks for it by duck type
 * (first-run/hold.ts). */
export interface SetupSource {
  /** Hears each SetupEvent: a new listener first gets SetupInfo, then each agent's install and sign-in state. Returns
   * the unsubscribe. */
  onSetup(listener: (event: SetupEvent) => void): () => void;
  /** Sends one setup command; rejects with the HostError when the host refuses it. */
  setup(command: SetupCommand): Promise<void>;
  /** Counts one event only the webview sees. */
  countUi(event: UiCount): Promise<void>;
}

/** The two plans' agent ids (crates/protocol/src/limits.rs). */
export const CLAUDE: AgentId = "claude-acp";
export const CODEX: AgentId = "codex-acp";

/** One agent's setup, as the window has heard it. */
export type AgentSetup = {
  install?: InstallProgress;
  signIn?: SignIn;
  /** The download's size in MB, as the host last said it (missing's download_bytes, downloading's total). */
  mb?: number;
  /** The install became ready while this window watched: the first screen says so. */
  readyHere?: boolean;
  /** The sign-in finished while this window watched, or the first word was a reused login: the signed-in line shows. */
  signedHere?: boolean;
  /** The code of the last setup command desktop refused for it, until its next sign-in change or a press. */
  refused?: string | undefined;
};

export type CountsState = {
  /** The switch as it stands: the lawyer's before the first act, `privacy.counts` after. */
  on: boolean;
  /** The firm's policy locks `privacy.counts`: shown off, with no control. */
  locked: boolean;
  /** The first act has happened (this window's, or `privacy.counts_started`). */
  started: boolean;
};

export type SetupState = {
  /** Whether this computer can keep an API key; null before SetupInfo. */
  keys: boolean | null;
  agents: Readonly<Record<AgentId, AgentSetup>>;
  /** The plan the first screen shows getting ready: Claude's, until the lawyer chooses ChatGPT. */
  chosen: AgentId;
  /** "Use … instead" pressed: the agent the new conversation takes once the room lists it; null after. */
  pending: AgentId | null;
  /** The gate's own: the key panel open, the paste field open, what was copied for the failure on screen. */
  keyOpen: boolean;
  pasting: boolean;
  copied: "message" | "log" | null;
  counts: CountsState;
};

const START: SetupState = {
  keys: null,
  agents: {},
  chosen: CLAUDE,
  pending: null,
  keyOpen: false,
  pasting: false,
  copied: null,
  counts: { on: true, locked: false, started: false },
};

const MB = 1_000_000;

/** Whether an install state is a failure on screen, and its code. */
function installCode(state: InstallProgress): ErrorCode | null {
  if (typeof state === "string") return null;
  if ("waiting" in state && state.waiting !== undefined) return state.waiting.code;
  if ("failed" in state && state.failed !== undefined) return state.failed.code;
  return null;
}

export const isReady = (state: InstallProgress | undefined): boolean =>
  state !== undefined && typeof state === "object" && "ready" in state && state.ready !== undefined;

export const isSignedIn = (state: SignIn | undefined): boolean =>
  state !== undefined &&
  typeof state === "object" &&
  "signed_in" in state &&
  state.signed_in !== undefined;

export type SetupStore = {
  subscribe: (listener: () => void) => () => void;
  state: () => SetupState;
  /** Subscribes to the setup stream and reads the privacy settings; returns the stop. */
  start: () => () => void;
  /** One setup command; a refusal goes to the console and to the agent's `refused`, and the next event says what came
   * of it. */
  command: (command: SetupCommand) => void;
  /** `command`, with the promise of whether desktop took it: a code's or a key's field keeps its words until then. */
  send: (command: SetupCommand) => Promise<boolean>;
  /** Changes the gate's own state. */
  set: (
    patch: Partial<Pick<SetupState, "keyOpen" | "pasting" | "copied" | "chosen" | "pending">>,
  ) => void;
  /** The counts switch, pressed: before the first act it is local state only; after it, `set_privacy_settings`. */
  setCounts: (on: boolean) => void;
  /** The lawyer's first act: its count, then `first_act` with the switch's value, once. */
  firstAct: (step: UiOnboardingStep | null) => void;
  /** One count, once per window. */
  count: (event: UiCount) => void;
};

/** The setup store over a transport that carries the setup wire. */
export function createSetupStore(transport: HostTransport & SetupSource): SetupStore {
  let state = START;
  const listeners = new Set<() => void>();
  const counted = new Set<string>();
  let firstActSent = false;
  const emit = (next: SetupState) => {
    state = next;
    for (const listener of listeners) listener();
  };
  const warn = (what: string) => (error: unknown) =>
    console.warn(`${what} failed: ${JSON.stringify(error)}`);
  const count = (event: UiCount) => {
    const key = JSON.stringify(event);
    if (counted.has(key)) return;
    counted.add(key);
    transport.countUi(event).catch(warn("count_ui"));
  };
  const agent = (id: AgentId, patch: AgentSetup) =>
    emit({ ...state, agents: { ...state.agents, [id]: { ...state.agents[id], ...patch } } });
  const hear = (event: SetupEvent) => {
    if ("setup_info" in event && event.setup_info !== undefined) {
      emit({ ...state, keys: event.setup_info.keys_available });
    } else if ("agent_install_progress" in event && event.agent_install_progress !== undefined) {
      const { agent: id, state: install } = event.agent_install_progress;
      const was = state.agents[id];
      const patch: AgentSetup = { install };
      if (typeof install === "object" && "missing" in install && install.missing !== undefined)
        patch.mb = Math.round(install.missing.download_bytes / MB);
      if (
        typeof install === "object" &&
        "downloading" in install &&
        install.downloading !== undefined
      )
        patch.mb = Math.round(install.downloading.total / MB);
      if (isReady(install) && was?.install !== undefined && !isReady(was.install)) {
        patch.readyHere = true;
        count({ onboarding_step: { step: "agent_ready" } });
      }
      // what was copied belongs to the failure it was copied for
      const code = installCode(install);
      if (code !== null && (was?.install === undefined || installCode(was.install) !== code))
        emit({ ...state, copied: null });
      if (code !== null) count({ error_shown: { code } });
      agent(id, patch);
    } else if ("sign_in_changed" in event && event.sign_in_changed !== undefined) {
      const { agent: id, state: signIn } = event.sign_in_changed;
      const was = state.agents[id];
      // a new sign-in state answers whatever was refused before it
      const patch: AgentSetup = { signIn, refused: undefined };
      if (isSignedIn(signIn) && !isSignedIn(was?.signIn)) {
        // the first word a reused login: no sign-in step, and the through line (UD1's brief)
        const reused = typeof signIn === "object" && signIn.signed_in?.source === "reused";
        if (was?.signIn !== undefined || reused) patch.signedHere = true;
        if (was?.signIn !== undefined) count({ onboarding_step: { step: "signed_in" } });
      }
      if (typeof signIn === "object" && "failed" in signIn && signIn.failed !== undefined)
        count({ error_shown: { code: signIn.failed.code } });
      // a new state closes the paste field and the key panel unless the lawyer is still in that sign-in
      const keep =
        typeof signIn === "object" && "waiting" in signIn && signIn.waiting !== undefined;
      emit({
        ...state,
        pasting: keep ? state.pasting : false,
        keyOpen: isSignedIn(signIn) ? false : state.keyOpen,
      });
      agent(id, patch);
    }
  };
  const send = (command: SetupCommand): Promise<boolean> => {
    const id = Object.values(command)[0]?.agent;
    // a press answers whatever was refused before it
    if (id !== undefined && state.agents[id]?.refused !== undefined)
      agent(id, { refused: undefined });
    return transport.setup(command).then(
      () => true,
      (error: unknown) => {
        // the command's kind only: it can hold a pasted code or an API key
        warn(`setup ${Object.keys(command).join(",")}`)(error);
        const code = (error as { code?: unknown } | null)?.code;
        if (id !== undefined) agent(id, { refused: typeof code === "string" ? code : "internal" });
        return false;
      },
    );
  };
  const readSettings = async () => {
    try {
      const answer = await transport.query("get_settings");
      if (!("settings" in answer) || answer.settings === undefined) return;
      takeSettings(answer.settings);
    } catch {
      // a host that can't answer yet: the switch stays as it is, on until the lawyer turns it off
    }
  };
  const takeSettings = (settings: SettingsView) => {
    const locked = settings.locked.includes("privacy.counts");
    const started = settings.privacy.counts_started || firstActSent;
    emit({
      ...state,
      counts: {
        on: locked ? false : started ? settings.privacy.counts : state.counts.on,
        locked,
        started,
      },
    });
  };
  return {
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    state: () => state,
    start: () => {
      const stop = transport.onSetup(hear);
      void readSettings();
      return stop;
    },
    command: (command) => void send(command),
    send,
    set: (patch) => emit({ ...state, ...patch }),
    setCounts: (on) => {
      if (state.counts.locked) return;
      emit({ ...state, counts: { ...state.counts, on } });
      // before the first act nothing is saved or sent (S21): the switch is the window's alone
      if (!state.counts.started) return;
      transport
        .dispatch({
          command_id: crypto.randomUUID(),
          command: { set_privacy_settings: { patch: { counts: on } } },
        })
        .then(readSettings, warn("set_privacy_settings"));
    },
    firstAct: (step) => {
      if (step !== null) count({ onboarding_step: { step } });
      if (firstActSent || state.counts.started) return;
      firstActSent = true;
      const counts = state.counts.on && !state.counts.locked;
      emit({ ...state, counts: { ...state.counts, started: true } });
      transport
        .dispatch({ command_id: crypto.randomUUID(), command: { first_act: { counts } } })
        .catch(warn("first_act"));
    },
    count,
  };
}

const stores = new WeakMap<object, SetupStore>();

/** The transport's one setup store, made on first ask. */
export function setupStoreOf(transport: HostTransport & SetupSource): SetupStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = createSetupStore(transport);
    stores.set(transport, store);
  }
  return store;
}
