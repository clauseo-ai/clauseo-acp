// UD1's first run in the room, the lazy chunk room.tsx loads where the host has the setup wire: the setup store
// (store/setup.ts) and its subscription, the first screen while there is no matter yet, and the hold on the composer
// (hold.ts): the install's working line, the sign-in gate in the field's row until the conversation's plan is signed
// in, and the room's line under the field (the fine print until then, the signed-in line after). Each gate press is one
// setup command; nothing else here talks to the host but the matter's creation, the counts and the first act.
import { lazy, Suspense, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import {
  Gate,
  SignedLine,
  type GateAct,
  type Plan,
} from "@clauseo/components/clauseo/first-launch/gate.tsx";
import {
  installLine,
  type InstallActs,
} from "@clauseo/components/clauseo/first-launch/install.tsx";
import { DocumentsLine } from "@clauseo/components/clauseo/first-launch/parts.tsx";
import { Welcome } from "@clauseo/components/clauseo/first-launch/welcome.tsx";
import { useDisplay, type Lang } from "@clauseo/components/lib/display.ts";
import { toHostError, type HostTransport } from "../../transport/host-transport.ts";
import type { AgentId } from "../../transport/protocol.ts";
import {
  CLAUDE,
  CODEX,
  setupStoreOf,
  type AgentSetup,
  type SetupSource,
  type SetupState,
  type SetupStore,
} from "../../store/setup.ts";
import type { Conversations, ConversationsView } from "../conversations.ts";
import type { Matters } from "../matters.ts";
import { clockOf } from "../when.ts";
import { holderOf, type Hold } from "./hold.ts";
import { CaseLawSlot } from "./case-law.tsx";
import { workspaceStep } from "./workspace.ts";
import { gateView, installView } from "./views.ts";

/** The test ids the brief decides (UD1, "Decided here"). */
const IDS = {
  install: {
    line: "install-foot",
    retry: "install-retry",
    proxy: "install-proxy",
    askIt: "install-ask-it",
  },
  gate: {
    root: "signin",
    state: "signin-state",
    cancel: "signin-cancel",
    paste: "signin-paste",
    code: "signin-code",
    deviceCode: "signin-device-code",
    key: "signin-key",
    loginFound: "login-found",
    loginUse: "login-use",
    loginOther: "login-other",
  },
  done: "signin-done",
  counts: "counts-switch",
} as const;

/** Each plan's prices page, for an account with no plan (H11's open_external asks first). */
const PLANS: Record<Plan, string> = {
  claude: "https://claude.com/pricing",
  chatgpt: "https://chatgpt.com/pricing",
};
const NAME: Record<Plan, string> = { claude: "Claude", chatgpt: "ChatGPT" };
const planOf = (agent: AgentId): Plan => (agent === CODEX ? "chatgpt" : "claude");

const copyText = (text: string) => void navigator.clipboard?.writeText(text).catch(() => {});

/** What a gate press does for `agent`. */
function actsFor(
  store: SetupStore,
  transport: HostTransport,
  agent: AgentId,
  device: string | null,
): (a: GateAct) => void {
  return (a) => {
    switch (a.act) {
      case "sign-in":
        store.set({ pasting: false, keyOpen: false });
        store.command({ start_sign_in: a.console === true ? { agent, console: true } : { agent } });
        return;
      case "cancel":
        store.set({ pasting: false });
        store.command({ cancel_sign_in: { agent } });
        return;
      case "paste":
        store.set({ pasting: true });
        return;
      case "code":
        // straight into the command, and nowhere else: the field cleared itself as it handed the code on
        store.set({ pasting: false });
        store.command({ paste_sign_in_code: { agent, code: a.code } });
        return;
      case "device":
        store.command({ use_device_code: { agent } });
        return;
      case "open-page":
        store.command({ open_sign_in_page: { agent } });
        return;
      case "copy-code":
        if (device !== null) copyText(device);
        return;
      case "use-found":
        store.command({ use_found_login: { agent } });
        return;
      case "different":
        store.command({ use_different_account: { agent } });
        return;
      case "instead": {
        // the other plan's install starts (Claude's download pauses where it is, R20), and the new conversation takes
        // it at once (FirstRun's pending choice)
        const other = agent === CODEX ? CLAUDE : CODEX;
        store.set({ chosen: other, pending: other, keyOpen: false, pasting: false });
        store.command({ install_agent: { agent: other } });
        return;
      }
      case "key-panel":
        store.set({ keyOpen: a.open });
        return;
      case "key":
        store.command({ set_agent_key: { agent, key: a.key } });
        return;
      case "plans":
        transport
          .openExternal(PLANS[planOf(agent)])
          .catch((error: unknown) =>
            console.warn(`open_external failed: ${JSON.stringify(error)}`),
          );
        return;
    }
  };
}

/** The install line's buttons for `agent`. */
const installActs = (store: SetupStore, agent: AgentId, copied: SetupState["copied"]) => ({
  retry: () => store.command({ retry_agent_install: { agent } }),
  proxy: () => store.command({ retry_agent_install: { agent, use_system_proxy: true } }),
  copy: copyText,
  copied,
  setCopied: (what: "message" | "log") => store.set({ copied: what }),
});

/** The room's side of the hold: the plan the send uses, and how times and words are said. */
type RoomSide = { agent: AgentId; zone: string; lang: Lang; platform: string };

/** The composer's hold for this setup state: the gate until the plan the send uses is signed in, the install's working
 * line, and the room's line under the field. A plain function of the store's state and the room's side, so the composer
 * reads it as the store changes, in the same commit as FirstRun's own (hold.ts). */
function holdOf(
  store: SetupStore,
  transport: HostTransport,
  state: SetupState,
  room: RoomSide,
): Hold | null {
  const { agent, lang } = room;
  const plan = planOf(agent);
  const setup = state.agents[agent];
  const clock = (at: number) => clockOf(at, Date.now(), room.zone, lang).time;
  const working = installLine(
    lang,
    installView(setup),
    NAME[plan],
    installActs(store, agent, state.copied),
    IDS.install,
  );
  const view = gateView(setup, state, clock);
  const signedIn = setup?.signIn;
  if (view !== null) {
    const device =
      typeof signedIn === "object" && "device_code" in signedIn
        ? (signedIn.device_code?.code ?? null)
        : null;
    const keys =
      state.keys === false
        ? null
        : room.platform === "mac" || room.platform === "windows"
          ? room.platform
          : "linux";
    return {
      gate: (
        <Gate
          plan={plan}
          view={view}
          keys={keys}
          onAct={actsFor(store, transport, agent, device)}
          testIds={IDS.gate}
        />
      ),
      working,
      line: <DocumentsLine name={NAME[plan]} />,
    };
  }
  // the Safe workspace step stands after the first sign-in, once UC merges (workspace.ts); none is needed today
  if (
    workspaceStep() === null &&
    typeof signedIn === "object" &&
    signedIn.signed_in !== undefined &&
    setup?.signedHere === true
  )
    return {
      working,
      line: (
        <SignedLine
          plan={plan}
          method={signedIn.signed_in.method}
          reused={signedIn.signed_in.source === "reused"}
          email={signedIn.signed_in.email}
          label={signedIn.signed_in.plan}
          testId={IDS.done}
        />
      ),
    };
  return working === null ? null : { working };
}

export function FirstRun({
  transport,
  matters,
  conversations,
  talks,
  zone,
  welcome,
}: {
  transport: HostTransport;
  matters: Matters;
  conversations: Conversations;
  talks: ConversationsView;
  /** The lawyer's zone, for the clock times the host gives. */
  zone: string;
  /** No matter yet: the first screen stands in the work's place. */
  welcome: boolean;
}) {
  const [store] = useState(() => setupStoreOf(transport as HostTransport & SetupSource));
  useEffect(() => store.start(), [store]);
  const state = useSyncExternalStore(store.subscribe, store.state, store.state);
  const { lang, platform } = useDisplay();
  // the plan the send uses: the open conversation's thread's, or the one a draft would run (room.tsx's openAgent)
  const agent = talks.rows.find((r) => r.key === talks.open)?.agent ?? talks.agent;
  // "Use … instead": the new conversation takes the other plan at once, listed or not (the chosen agent wins,
  // agent.ts's agentFor: codex-acp is listed only once its install is ready), and only once, so the lawyer's own pick
  // in the plan switch stands
  const pending = state.pending;
  const take = pending !== null && talks.drafting;
  useEffect(() => {
    if (!take || pending === null) return;
    store.set({ pending: null });
    conversations.choose(pending);
  }, [take, pending, store, conversations]);
  // the composer reads its hold from the setup store's state as it changes (one commit per event, with this one),
  // and again when the room's side changes
  useEffect(() => {
    const holder = holderOf(transport);
    const room: RoomSide = { agent, zone, lang, platform };
    let cache: [SetupState, Hold | null] | null = null;
    holder.read = () => {
      const now = store.state();
      if (cache === null || cache[0] !== now) cache = [now, holdOf(store, transport, now, room)];
      return cache[1];
    };
    const notify = () => {
      for (const heard of holder.heard) heard();
    };
    const stop = store.subscribe(notify);
    notify();
    return () => {
      stop();
      holder.read = () => null;
      notify();
    };
  }, [transport, store, agent, zone, lang, platform]);
  // a matter made from the rail while the first screen stood is the first act too
  const wasWelcome = useRef(welcome);
  useEffect(() => {
    if (wasWelcome.current && !welcome && matters.view().matters.some((m) => m.kind !== "scratch"))
      store.firstAct(null);
    wasWelcome.current = welcome;
  }, [welcome, matters, store]);
  if (!welcome) return <CaseLawSlot />;
  return (
    <FirstScreen
      store={store}
      matters={matters}
      transport={transport}
      plan={planOf(state.chosen)}
      setup={state.agents[state.chosen]}
      state={state}
      acts={installActs(store, state.chosen, state.copied)}
    />
  );
}

/** U2's new matter form in place, and a refused folder's words: New matter's chunk (new-matter.tsx), loaded as the
 * first screen needs them. */
const loadNewMatter = () => late(import("../new-matter.tsx"));
const FirstMatter = lazy(() => loadNewMatter().then((m) => ({ default: m.FirstMatter })));
const RefusedLine = lazy(() => loadNewMatter().then((m) => ({ default: m.RefusedLine })));

/** The folder's own name, the last part of how it reads. */
const folderName = (display: string) =>
  display
    .split(/[\\/]/)
    .filter((p) => p !== "")
    .pop() ?? display;

/** The first screen: the question, the install at its foot, the counts switch. Its acts make the first matter: a
 * folder chosen opens it at once (the room opens the moment it is made); a new matter takes U2's form, in place. */
function FirstScreen({
  store,
  matters,
  transport,
  plan,
  setup,
  state,
  acts,
}: {
  store: SetupStore;
  matters: Matters;
  transport: HostTransport;
  plan: Plan;
  setup: AgentSetup | undefined;
  state: SetupState;
  acts: InstallActs;
}) {
  const { lang } = useDisplay();
  const install = installLine(lang, installView(setup), NAME[plan], acts, IDS.install);
  const [form, setForm] = useState(false);
  const [refused, setRefused] = useState<{ code: string; path: string } | null>(null);
  useEffect(() => store.count({ onboarding_step: { step: "welcome_shown" } }), [store]);
  const openFolder = async () => {
    setRefused(null);
    const picked = await matters.pickFolder();
    if (picked === null) return;
    store.firstAct("folder_chosen");
    try {
      await matters.create(folderName(picked.display), {
        existing_folder: { folder: picked.folder },
      });
    } catch (thrown) {
      setRefused({ code: toHostError(thrown).code, path: picked.display });
    }
  };
  const openSettings = transport.openSettings;
  return (
    <Welcome
      name={NAME[plan]}
      install={install}
      ready={setup?.readyHere === true}
      counts={{ ...state.counts, onChange: store.setCounts }}
      onOpenFolder={() => void openFolder()}
      onNewMatter={() => {
        setRefused(null);
        setForm(true);
      }}
      {...(form
        ? {
            form: (
              <Suspense fallback={null}>
                <FirstMatter
                  matters={matters}
                  onAct={() => store.firstAct("new_matter")}
                  onBack={() => setForm(false)}
                />
              </Suspense>
            ),
          }
        : {})}
      error={
        refused === null ? null : (
          <Suspense fallback={null}>
            <RefusedLine code={refused.code} path={refused.path} />
          </Suspense>
        )
      }
      onWhatsSent={
        openSettings === undefined
          ? undefined
          : () => void openSettings.call(transport).catch(() => {})
      }
      testIds={{ counts: IDS.counts, open: "open-folder", newMatter: "start-new-matter" }}
    />
  );
}
