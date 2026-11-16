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
  PythonProgress,
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
  /** Its waiting sign-in was cancelled while it downloads, and the download carries on: its row says so, with no
   * Cancel, until the download ends (ready, failed or paused), a sign-in starts, or the lawyer presses Sign in. */
  stillDownloading?: boolean | undefined;
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
  /** The document tools' runtime, one for the computer (SetupInfo's `python`); null before SetupInfo and from an older
   * host that leaves it out. Its failure never changes a plan's state. */
  python: PythonProgress | null;
  /** The document tools' download in MB, as the host last said it (missing's download_bytes, downloading's total). */
  pythonMb?: number;
  agents: Readonly<Record<AgentId, AgentSetup>>;
  /** The plan the lawyer means: the last one whose Sign in they pressed. Before any press (`picked` false) it follows
   * the one plan that says it is signed in, so a lawyer with ChatGPT alone is not taken for Claude after a restart;
   * Claude's otherwise, as the fallback. The first read and the Safe workspace step follow it. */
  chosen: AgentId;
  /** The lawyer pressed a plan's Sign in (or "Use … instead") in this window: `chosen` no longer follows the sign-ins. */
  picked: boolean;
  /** A plan whose Sign in was pressed while the other plan's sign-in was under way: downloads run side by side, one
   * per plan, but sign-ins go one at a time, so its download starts at once and its start_sign_in waits here until the
   * other's sign-in ends (signed in, cancelled or failed), then goes by itself. Null with none waiting. */
  queued: AgentId | null;
  /** "Use … instead" pressed: the agent the new conversation takes once the room lists it; null after. */
  pending: AgentId | null;
  /** The gate's own: the key panel open, the paste field open, what was copied for the failure on screen. */
  keyOpen: boolean;
  pasting: boolean;
  copied: "log" | null;
  counts: CountsState;
  /** The settings the firm's policy locks (SettingsView.locked), as read at the start: a locked plan's sign-in has no
   * control on the first screen. */
  locked: readonly string[];
};

/** Whether a firm's policy locks the agent's plan: `plans.<id>`, or a prefix of it ending in `.` (`plans.`, while a
 * policy source can't be read; SettingsView.locked). */
export const planLocked = (locked: readonly string[], agent: AgentId): boolean => {
  const key = `plans.${agent}`;
  return locked.some((k) => k === key || (k.endsWith(".") && key.startsWith(k)));
};

const START: SetupState = {
  keys: null,
  python: null,
  agents: {},
  chosen: CLAUDE,
  picked: false,
  queued: null,
  pending: null,
  keyOpen: false,
  pasting: false,
  copied: null,
  counts: { on: true, locked: false, started: false },
  locked: [],
};

const MB = 1_000_000;

/** A byte count as the first run says a size: whole MB, 10^6 bytes, rounded. The one MB
 * formatter the install lines use (the download's size, and its done and total). */
export const mbOf = (bytes: number): number => Math.round(bytes / MB);

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

/** Whether the plan is signed in with the lawyer's API key. */
export const keyInUse = (state: SignIn | undefined): boolean =>
  isSignedIn(state) &&
  typeof state === "object" &&
  state.signed_in !== undefined &&
  state.signed_in.method === "api_key";

/** Whether the provider refused the API key in use: the host's `signed_in.refused` (#984), set after a question failed
 * and cleared when a key is saved or removed; false for a plan login or the firm's account. */
export const keyRefused = (state: SignIn | undefined): boolean => {
  if (!keyInUse(state) || typeof state !== "object" || state.signed_in === undefined) return false;
  return state.signed_in.refused !== undefined;
};

/** Whether a plan isn't installed here yet, and the firm's install doesn't stand in for it: missing, paused behind the
 * other plan's download, downloading, or a download that stopped. */
export const awaitsInstall = (setup: AgentSetup | undefined): boolean => {
  const install = setup?.install;
  return install !== undefined && install !== "managed" && !isReady(install);
};

/** A key was just saved for `agent`: a plan not here yet starts its normal download (install_agent), so the next
 * question can go once it is ready. A download already running, paused behind the other plan's, or stopped (its line
 * offers Try again) is left as it is. */
export function installForKey(store: Pick<SetupStore, "state" | "command">, agent: AgentId): void {
  const install = store.state().agents[agent]?.install;
  if (typeof install === "object" && "missing" in install && install.missing !== undefined)
    store.command({ install_agent: { agent } });
}

/** The plans, in the order every screen offers them: Claude, then ChatGPT. */
export const PLANS: readonly AgentId[] = [CLAUDE, CODEX];

/** A sign-in queued, being checked, or under way in the browser or with a code. */
function signInBusy(signIn: SignIn | undefined): boolean {
  return signIn === "queued" || signInGoing(signIn);
}

/** A sign-in that holds the one sign-in at a time: being checked, or under way in the browser, with a code or
 * switching accounts. A sign-in queued behind its own download (or behind the other plan's sign-in) doesn't, nor does a
 * login found, which waits on the lawyer's press. */
function signInGoing(signIn: SignIn | undefined): boolean {
  if (signIn === "checking") return true;
  return (
    typeof signIn === "object" &&
    (signIn.waiting !== undefined ||
      signIn.device_code !== undefined ||
      signIn.switching !== undefined)
  );
}

/** The other plan, of the two. */
export const otherPlan = (agent: AgentId): AgentId => (agent === CODEX ? CLAUDE : CODEX);

/** The plan whose sign-in is under way ahead of `agent`'s, which waits for it (sign-ins go one at a time); null with
 * none. */
export function signInAhead(state: SetupState, agent: AgentId): AgentId | null {
  const other = otherPlan(agent);
  return signInGoing(state.agents[other]?.signIn) ? other : null;
}

/** Whether a plan's install is under way: downloading, setting up, or waiting to reconnect. */
/** Whether an install is downloading or installing now (not waiting offline, failed or paused). */
const downloading = (install: InstallProgress | undefined): boolean =>
  install === "installing" || (typeof install === "object" && install.downloading !== undefined);

export function installGoing(setup: AgentSetup | undefined): boolean {
  const install = setup?.install;
  if (install === "installing") return true;
  return (
    typeof install === "object" &&
    (install.downloading !== undefined || install.waiting !== undefined)
  );
}

/** Whether a plan is installed: ready here, or by the firm. */
const installedOf = (setup: AgentSetup | undefined): boolean =>
  setup?.install === "managed" || isReady(setup?.install);

/** Whether a plan's sign-in waits: in this window's queue, or queued at the host (behind its own download, or behind
 * the other plan's sign-in). */
export function signInWaits(state: SetupState, agent: AgentId): boolean {
  if (isSignedIn(state.agents[agent]?.signIn)) return false;
  return state.queued === agent || state.agents[agent]?.signIn === "queued";
}

/** Whether a plan's sign-in is already under way or waits on the lawyer's press (a login found): a new start_sign_in
 * would only start it over. */
const signingIn = (setup: AgentSetup | undefined): boolean => {
  const signIn = setup?.signIn;
  return signInBusy(signIn) || (typeof signIn === "object" && signIn.login_found !== undefined);
};

/** The plan `chosen` follows before the lawyer picks: the one plan signed in, when exactly one is; else as it was. */
function follow(state: SetupState): SetupState {
  if (state.picked) return state;
  const signed = PLANS.filter((id) => isSignedIn(state.agents[id]?.signIn));
  const only = signed.length === 1 ? signed[0] : undefined;
  return only === undefined || only === state.chosen ? state : { ...state, chosen: only };
}

/** The one plan in play for the papers line (UD1-23 names it; UD1-93 names both): the plan the lawyer picked, else the
 * only one signed in; null while both are offered, nothing picked and neither signed in (or both are). */
export function planInPlay(state: SetupState): AgentId | null {
  if (state.picked) return state.chosen;
  const signed = PLANS.filter((id) => isSignedIn(state.agents[id]?.signIn));
  return signed.length === 1 ? (signed[0] ?? null) : null;
}

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
  /** A plan's "Sign in with {name}" (or the gate's "Use … instead"), pressed: it becomes the chosen plan (and the new
   * conversation's), and its sign-in goes through `signIn`. */
  signInTo: (agent: AgentId) => void;
  /** Every sign-in press goes here (the first screen's, the gate's Sign in and Try again, "Use … instead"): the
   * start_sign_in goes now, and desktop downloads the plan first if it isn't here, whatever the other plan's download
   * is doing; while the other plan's sign-in is under way, the plan's download starts now and its start_sign_in waits
   * in `queued` until that sign-in ends, then goes by itself. A plan already signed in, or whose sign-in is under way
   * or queued at desktop, sends nothing, unless `again` (the gate's own Sign in and Try again, which start a sign-in
   * under way over). `console`: Claude's Console sign-in, as the gate offers it. */
  signIn: (agent: AgentId, how?: { console?: boolean; again?: boolean }) => void;
  /** The waiting plan's Cancel: its sign-in waits no more, and nothing is sent; its download carries on. */
  unqueue: () => void;
  /** A plan's Cancel on its waiting sign-in: the store's own wait ends with nothing sent, or cancel_sign_in goes. Pressed
   * while the plan downloads, the sign-in goes but the download carries on (desktop drops only the kept press), so
   * once that is done the plan reads signed out, `stillDownloading`. */
  cancelSignIn: (agent: AgentId) => void;
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
    state = follow(next);
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
  const setAgent = agent;
  /** The Cancel on a waiting sign-in, as `cancelSignIn` says. */
  const cancelSignIn = (id: AgentId) => {
    const going = downloading(state.agents[id]?.install);
    const dropped: AgentSetup = { signIn: { signed_out: {} }, stillDownloading: true };
    if (state.queued === id) {
      emit({ ...state, queued: null, pasting: false });
      if (going) agent(id, dropped);
      return;
    }
    const kept = state.agents[id]?.signIn === "queued";
    emit({ ...state, pasting: false });
    void send({ cancel_sign_in: { agent: id } }).then((ok) => {
      // desktop dropped the kept press, and says signed out as it does; the row says so at once
      if (ok && going && kept && downloading(state.agents[id]?.install)) agent(id, dropped);
    });
  };
  // the plans whose start_sign_in went, on an installed plan, and whose host hasn't said anything of them since: their
  // sign-in is about to be under way, so the other plan's press in that moment waits for it too
  const starting = new Set<AgentId>();
  // the waiting plan's sign-in is Claude's Console one
  let queuedConsole = false;
  /** Whether a sign-in other than `agent`'s is under way: `agent`'s own start_sign_in waits for it. */
  const otherSigningIn = (agent: AgentId): boolean =>
    [...starting].some((id) => id !== agent) || signInAhead(state, agent) !== null;
  const startSignIn = (agent: AgentId, console: boolean) => {
    if (installedOf(state.agents[agent])) starting.add(agent);
    void send({ start_sign_in: console ? { agent, console: true } : { agent } }).then((ok) => {
      if (ok) return;
      // refused: nothing started, so a plan waiting on it goes now
      starting.delete(agent);
      release();
    });
  };
  /** A waiting plan's download, started now: a plan not here yet, and not downloading already. Downloads run side by
   * side, so the other plan's is never touched. */
  const startDownload = (agent: AgentId) => {
    const setup = state.agents[agent];
    const install = setup?.install;
    if (install === undefined || installedOf(setup) || installGoing(setup)) return;
    if (install === "paused" || (typeof install === "object" && install.failed !== undefined))
      void send({ retry_agent_install: { agent } });
    else void send({ install_agent: { agent } });
  };
  /** The waiting plan's start_sign_in, once no other sign-in is under way; a waiting plan whose download failed waits
   * no more (its line offers Try again, and its Sign in after). */
  const release = () => {
    const waiting = state.queued;
    if (waiting === null) return;
    const install = state.agents[waiting]?.install;
    if (typeof install === "object" && install.failed !== undefined) {
      emit({ ...state, queued: null });
      return;
    }
    if (otherSigningIn(waiting)) return;
    emit({ ...state, queued: null });
    const setup = state.agents[waiting];
    if (!isSignedIn(setup?.signIn) && !signingIn(setup)) startSignIn(waiting, queuedConsole);
  };
  const hear = (event: SetupEvent) => {
    const id = event.agent_install_progress?.agent ?? event.sign_in_changed?.agent ?? null;
    if (id !== null) starting.delete(id);
    heard(event);
    release();
  };
  const signIn = (agent: AgentId, how: { console?: boolean; again?: boolean } = {}) => {
    const console = how.console === true;
    if (state.agents[agent]?.stillDownloading === true)
      setAgent(agent, { stillDownloading: undefined });
    if (state.queued === agent) emit({ ...state, queued: null });
    // signed in, or its sign-in under way or queued at desktop already: nothing to send, but the gate's own press,
    // which starts it over
    const setup = state.agents[agent];
    if (isSignedIn(setup?.signIn) || (how.again !== true && signingIn(setup))) return;
    if (otherSigningIn(agent)) {
      queuedConsole = console;
      emit({ ...state, queued: agent });
      startDownload(agent);
      return;
    }
    startSignIn(agent, console);
  };
  const heard = (event: SetupEvent) => {
    if ("setup_info" in event && event.setup_info !== undefined) {
      const python = event.setup_info.python ?? null;
      const mb =
        typeof python === "object" && python?.missing !== undefined
          ? mbOf(python.missing.download_bytes)
          : typeof python === "object" && python?.downloading !== undefined
            ? mbOf(python.downloading.total)
            : state.pythonMb;
      emit({
        ...state,
        keys: event.setup_info.keys_available,
        python,
        ...(mb === undefined ? {} : { pythonMb: mb }),
      });
    } else if ("agent_install_progress" in event && event.agent_install_progress !== undefined) {
      const { agent: id, state: install } = event.agent_install_progress;
      const was = state.agents[id];
      const patch: AgentSetup = { install };
      if (!downloading(install)) patch.stillDownloading = undefined;
      if (typeof install === "object" && "missing" in install && install.missing !== undefined)
        patch.mb = mbOf(install.missing.download_bytes);
      if (
        typeof install === "object" &&
        "downloading" in install &&
        install.downloading !== undefined
      )
        patch.mb = mbOf(install.downloading.total);
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
      if (signInBusy(signIn) || isSignedIn(signIn)) patch.stillDownloading = undefined;
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
      locked: settings.locked,
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
    // a plan chosen by hand ("Use … instead") is the lawyer's pick
    set: (patch) =>
      emit({ ...state, ...patch, picked: state.picked || patch.chosen !== undefined }),
    signInTo: (agent) => {
      emit({
        ...state,
        chosen: agent,
        picked: true,
        pending: agent,
        keyOpen: false,
        pasting: false,
      });
      // signed in, or its sign-in under way already: it only becomes the chosen plan
      signIn(agent);
    },
    signIn,
    unqueue: () => emit({ ...state, queued: null }),
    cancelSignIn,
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
