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
import { useInstallLine } from "@clauseo/components/clauseo/first-launch/install.tsx";
import { DocumentsLine } from "@clauseo/components/clauseo/first-launch/parts.tsx";
import { Welcome } from "@clauseo/components/clauseo/first-launch/welcome.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
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
import { useNow } from "../../store/limits.ts";

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
  choose: (agent: AgentId) => void,
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
        // the other plan's install starts (Claude's download pauses where it is, R20), and new conversations take it
        const other = agent === CODEX ? CLAUDE : CODEX;
        store.set({ chosen: other, keyOpen: false, pasting: false });
        store.command({ install_agent: { agent: other } });
        choose(other);
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
  const now = useNow();
  const clock = (at: number) => clockOf(at, now, zone, lang).time;
  // the plan the gate is for: a new conversation's is the one the lawyer chose here (Claude's until "Use ChatGPT
  // instead"), once the host has said anything of it; a conversation with a thread keeps its own
  const drafting = talks.drafting || talks.open === null;
  const agent =
    drafting && state.agents[state.chosen] !== undefined
      ? state.chosen
      : (talks.rows.find((r) => r.key === talks.open)?.agent ?? talks.agent);
  const plan = planOf(agent);
  const setup = state.agents[agent];
  const acts = {
    retry: () => store.command({ retry_agent_install: { agent } }),
    proxy: () => store.command({ retry_agent_install: { agent, use_system_proxy: true } }),
    copy: copyText,
    copied: state.copied,
    setCopied: (copied: "message" | "log") => store.set({ copied }),
  };
  const working = useInstallLine(installView(setup), NAME[plan], acts, IDS.install);
  const view = gateView(setup, state, clock);
  // the Safe workspace step stands after the first sign-in, once UC merges (seams.tsx); none is needed today
  const step = workspaceStep();
  const signedIn = setup?.signIn;
  const choose = (other: AgentId) => {
    if (talks.drafting && talks.options.some((o) => o.value === other)) conversations.choose(other);
  };
  // the chosen plan's agent may be listed only once its install is ready (its settings entry is written then): the
  // new conversation takes it as soon as the room offers it
  const offered = talks.drafting && talks.options.some((o) => o.value === state.chosen);
  useEffect(() => {
    if (offered && talks.agent !== state.chosen) conversations.choose(state.chosen);
  }, [offered, talks.agent, state.chosen, conversations]);
  const device =
    typeof signedIn === "object" && "device_code" in signedIn
      ? (signedIn.device_code?.code ?? null)
      : null;
  const onAct = actsFor(store, transport, agent, device, choose);
  const keys =
    state.keys === false ? null : platform === "mac" || platform === "windows" ? platform : "linux";
  let hold: Hold | null = null;
  if (view !== null)
    hold = {
      gate: <Gate plan={plan} view={view} keys={keys} onAct={onAct} testIds={IDS.gate} />,
      working,
      line: <DocumentsLine name={NAME[plan]} />,
    };
  else if (
    step === null &&
    typeof signedIn === "object" &&
    signedIn.signed_in !== undefined &&
    setup?.signedHere === true
  )
    hold = {
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
  else if (working !== null) hold = { working };
  // the composer hears a new hold only when what it says changes: a room render alone (a streaming answer's) sends
  // nothing, so the composer renders no more than before
  const said = JSON.stringify([
    agent,
    setup,
    view,
    state.copied,
    state.keys,
    lang,
    platform,
    talks.drafting,
  ]);
  const last = useRef<string | null>(null);
  useEffect(() => {
    if (last.current === said) return;
    last.current = said;
    const holder = holderOf(transport);
    holder.hold = hold;
    for (const heard of holder.heard) heard();
  });
  useEffect(
    () => () => {
      const holder = holderOf(transport);
      holder.hold = null;
      for (const heard of holder.heard) heard();
    },
    [transport],
  );
  // a matter made from the rail while the first screen stood is the first act too
  const wasWelcome = useRef(welcome);
  useEffect(() => {
    if (wasWelcome.current && !welcome && matters.view().matters.some((m) => m.kind !== "scratch"))
      store.firstAct(null);
    wasWelcome.current = welcome;
  }, [welcome, matters, store]);
  if (!welcome) return <CaseLawSlot />;
  const shown = state.agents[state.chosen];
  return (
    <FirstScreen
      store={store}
      matters={matters}
      transport={transport}
      plan={planOf(state.chosen)}
      setup={shown}
      state={state}
      acts={{
        ...acts,
        retry: () => store.command({ retry_agent_install: { agent: state.chosen } }),
        proxy: () =>
          store.command({ retry_agent_install: { agent: state.chosen, use_system_proxy: true } }),
      }}
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
  acts: Parameters<typeof useInstallLine>[2];
}) {
  const install = useInstallLine(installView(setup), NAME[plan], acts, IDS.install);
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
