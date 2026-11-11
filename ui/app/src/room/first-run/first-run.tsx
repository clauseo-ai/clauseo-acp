// UD1's first run in the room, in the first load, which room.tsx renders where the host has the setup wire: the setup store
// (store/setup.ts) and its subscription, the first screen while there is no matter yet, and the hold on the composer
// (hold.ts): the install's working line, the sign-in gate in the field's row until the conversation's plan is signed
// in, and the room's line under the field (the fine print until then, the signed-in line after). Each gate press is one
// setup command; nothing else here talks to the host but the matter's creation, the counts and the first act.
import { lazy, Suspense, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { Gate, SignedLine, type GateAct } from "@clauseo/components/clauseo/first-launch/gate.tsx";
import { installLine } from "@clauseo/components/clauseo/first-launch/install.tsx";
import { DocumentsLine } from "@clauseo/components/clauseo/first-launch/parts.tsx";
import { useDisplay, type Lang, type ReleaseMode } from "@clauseo/components/lib/display.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId } from "../../transport/protocol.ts";
import {
  CLAUDE,
  CODEX,
  awaitsInstall,
  installForKey,
  installGoing,
  isSignedIn,
  keyInUse,
  otherPlan,
  planInPlay,
  signInWaits,
  setupStoreOf,
  type SetupSource,
  type SetupState,
  type SetupStore,
} from "../../store/setup.ts";
import type { Conversations, ConversationsView } from "../conversations.ts";
import type { Matters } from "../matters.ts";
import { clockOf } from "../when.ts";
import { firstReadMarks, markAdded, subscribeMarks } from "../first-read/marks.ts";

/** The first read's offer, in its own chunk with the first read's words (first-read-offer.tsx). */
const FirstReadOffer = lazy(() =>
  late(import("../first-read/first-read-offer.tsx")).then((m) => ({ default: m.FirstReadOffer })),
);

/** The first read's title, as start.ts names the thread (words-first-read.ts's UD2-12, in English as the host keeps it). */
const FIRST_READ_TITLE_EN = "First read";
import { holderOf, provideExpiredCard, provideOffer, type Hold } from "./hold.ts";
import { ExpiredCard } from "../signin-expired/expired-card.tsx";
import { CaseLawSlot } from "./case-law.tsx";
import { stepAllowsRead, stepDue, workspaceStep } from "./workspace.ts";
import {
  carriesProtection,
  protectionStoreOf,
  type ProtectionState,
  type ProtectionStore,
} from "../../store/protection.ts";
import { loadProtectionNote } from "../protection/note-load.ts";
import { copyText, IDS, installActs, NAME, PLANS, planOf } from "./acts.ts";
import { FirstScreen } from "./first-screen.tsx";
import { repairOf, stepWords, type StepParts } from "./step-parts.ts";
import { gateView, installView, queuedView, refusalView } from "./views.ts";
import { footActs, stillNote, welcomeFoot } from "./welcome-foot.tsx";

// UD2: the expired sign-in card rides here, with the setup store and the gate; the thread's chunk places it
provideExpiredCard((props) => <ExpiredCard {...props} />);

/** What a gate press does for `agent`. */
function actsFor(
  store: SetupStore,
  transport: HostTransport,
  agent: AgentId,
  device: string | null,
): (a: GateAct) => void | Promise<boolean> {
  return (a) => {
    switch (a.act) {
      case "sign-in":
        // Sign in and Try again, through the store's queue: it waits for the other plan's sign-in, never its download
        store.set({ pasting: false, keyOpen: false });
        store.signIn(agent, { console: a.console === true, again: true });
        return;
      case "sign-in-to": {
        // either plan's own Sign in, through the store's queue: the other plan's choice moves the new conversation to
        // it as "Use … instead" did. A plan with its key saved and not installed yet installs instead (no sign-in)
        const to = a.plan === "chatgpt" ? CODEX : CLAUDE;
        if (keyInUse(store.state().agents[to]?.signIn)) {
          store.command({ install_agent: { agent: to } });
          return;
        }
        store.signInTo(to);
        return;
      }
      case "cancel":
        // the plan waiting its turn stops waiting (nothing sent), or cancel_sign_in goes; its download carries on
        store.cancelSignIn(agent);
        return;
      case "paste":
        store.set({ pasting: true });
        return;
      case "code":
        // straight into the command, and nowhere else: the field keeps it until desktop takes it, then clears; after a
        // refusal it stays for a retry
        return store.send({ paste_sign_in_code: { agent, code: a.code } }).then((ok) => {
          if (ok) store.set({ pasting: false });
          return ok;
        });
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
      case "instead":
        // the other plan's own Sign in, through the store's queue: its download starts beside this plan's, which goes
        // on, and the new conversation takes it at once (FirstRun's pending choice)
        store.signInTo(otherPlan(agent));
        return;
      case "key-panel":
        store.set({ keyOpen: a.open });
        return;
      case "key":
        // a saved key on a plan not here yet starts its download at once, so the next question can go
        return store.send({ set_agent_key: { agent, key: a.key } }).then((ok) => {
          if (ok) installForKey(store, agent);
          return ok;
        });
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

const noSubscribe = () => () => {};

/** The room's side of the hold: the plan the send uses, and how times and words are said. */
type RoomSide = {
  agent: AgentId;
  /** The open conversation is a thread already: its plan is fixed. */
  fixed: boolean;
  /** The open conversation is a draft, whose plan can still change. */
  drafting: boolean;
  zone: string;
  lang: Lang;
  platform: string;
  releaseMode: ReleaseMode;
};

/** UC: the Safe workspace step's acts over the window's protection store. */
function stepActs(transport: HostTransport, store: ProtectionStore, parts: StepParts) {
  return {
    setUp: () =>
      void parts.retryProtection(transport, store).then(() => store.set({ step: "done" })),
    notNow: () => store.set({ step: "done" }),
    retry: () => store.set({ step: "ask", failed: null }),
    settings:
      transport.openSettings === undefined
        ? undefined
        : () => void transport.openSettings?.("protection").catch(() => {}),
  };
}

/** The gate's view for the plan the send uses. The plan signed out where the other is signed in, or in a conversation
 * fixed to it, is the set-up view: its own one act ("Set up {name}", or "Sign in again" after an expired sign-in), and
 * on a draft the other plan as a quiet way round; with neither plan signed in on a draft, both stand side by side. */
function viewFor(state: SetupState, room: RoomSide, clock: (at: number) => string) {
  const { agent, drafting } = room;
  const setup = state.agents[agent];
  // its API key is saved but the plan isn't here yet: nothing can go until it is installed, so the set-up view holds the
  // question (its act installs; busy while the download runs, which the working line follows), never "Sign in"
  if (keyInUse(setup?.signIn) && awaitsInstall(setup)) {
    const install = setup?.install;
    // not started (missing, or paused behind the other plan's download): its act starts it; else it runs, or stopped
    // with the working line's own Try again
    const idle =
      install === "paused" ||
      (typeof install === "object" && "missing" in install && install.missing !== undefined);
    return { kind: "set-up", expired: false, instead: drafting, keyed: true, busy: !idle } as const;
  }
  if (state.queued === agent && !isSignedIn(setup?.signIn)) return queuedView(setup, state, agent);
  const view = gateView(setup, state, clock, agent);
  if (view?.kind !== "signed-out" || view.found !== undefined) return view;
  const other = isSignedIn(state.agents[agent === CODEX ? CLAUDE : CODEX]?.signIn);
  if (drafting && !other) return view;
  const signIn = setup?.signIn;
  const expired = typeof signIn === "object" && signIn.failed?.code === "agent.auth_required";
  return { kind: "set-up", expired, instead: drafting } as const;
}

/** The other plan's line in the composer, as the first screen says it (its name in front, its own Cancel), while its
 * install is under way, failed or offline, or its sign-in waits or is open in the browser; null otherwise (only
 * offered, ready or signed in). */
function otherLine(store: SetupStore, state: SetupState, other: AgentId, lang: Lang) {
  const setup = state.agents[other];
  const view = installView(setup);
  const signIn = setup?.signIn;
  const moving =
    installGoing(setup) ||
    (view !== null && view.kind !== "missing") ||
    signInWaits(state, other) ||
    (typeof signIn === "object" && signIn.waiting !== undefined);
  if (!moving) return null;
  return welcomeFoot(
    lang,
    other,
    NAME[planOf(other)],
    setup,
    state,
    footActs(store, other, state),
    IDS.install,
  );
}

/** The composer's hold for this setup state: the gate until the plan the send uses is signed in, the install's working
 * line, and the room's line under the field. A plain function of the store's state and the room's side, so the composer
 * reads it as the store changes, in the same commit as FirstRun's own (hold.ts). */
function holdOf(
  store: SetupStore,
  transport: HostTransport,
  state: SetupState,
  room: RoomSide,
  shield: { store: ProtectionStore; state: ProtectionState; parts: StepParts } | null,
): Hold | null {
  const { agent, lang } = room;
  const plan = planOf(agent);
  const setup = state.agents[agent];
  const clock = (at: number) => clockOf(at, Date.now(), room.zone, lang).time;
  const line = installLine(
    lang,
    installView(setup),
    NAME[plan],
    installActs(store, agent, state.copied),
    IDS.install,
  );
  // its sign-in cancelled while it downloads: the note under its line, as the first screen's row has it
  const note = stillNote(lang, NAME[plan], setup);
  const own = line?.kind === "wait" && note !== null ? { ...line, note } : line;
  // the other plan's line while it downloads or its sign-in waits, compact under this plan's: choosing a plan never
  // hides the other's download, and each line keeps its own progress and Cancel
  const other = otherLine(store, state, otherPlan(agent), lang);
  const working = own ?? other;
  const also = own === null ? null : other;
  // a plan waiting for the other's download or sign-in to end says the queued sign-in's words, with its Cancel
  const view = viewFor(state, room, clock);
  const signedIn = setup?.signIn;
  if (view !== null) {
    const device =
      typeof signedIn === "object" && "device_code" in signedIn
        ? (signedIn.device_code?.code ?? null)
        : null;
    // one plan in play (the thread's, the picked one, or the only one signed in) is named; else both are
    const one = room.fixed ? agent : planInPlay(state);
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
          refused={refusalView(setup)}
          testIds={IDS.gate}
        />
      ),
      working,
      also,
      line: <DocumentsLine name={one === null ? null : NAME[planOf(one)]} />,
    };
  }
  // the Safe workspace step stands after the first sign-in (UC, workspace.ts): in the gate's place while it asks or
  // waits, and as the room's line once it ended without protection
  const signedHere =
    typeof signedIn === "object" && signedIn.signed_in !== undefined && setup?.signedHere === true;
  const step = signedHere ? workspaceStep(shield?.state ?? null, shield?.parts ?? null) : null;
  if (
    shield !== null &&
    (step === "repair" || step === "ask" || step === "waiting" || step === "wait")
  ) {
    const acts = stepActs(transport, shield.store, shield.parts);
    const { WorkspaceStep } = shield.parts;
    const repair = repairOf(shield.state, shield.parts, lang, { releaseMode: room.releaseMode });
    const page = repair.page;
    return {
      gate: (
        <WorkspaceStep
          state={step}
          words={stepWords(lang, shield.parts.pw)}
          repair={repair.lines}
          onGetInstaller={
            page === null ? undefined : () => void shield.parts.openInstallerPage(transport, page)
          }
          onSetUp={acts.setUp}
          onNotNow={acts.notNow}
        />
      ),
      working,
      also,
    };
  }
  if (shield !== null && step === "off") {
    const acts = stepActs(transport, shield.store, shield.parts);
    const { WorkspaceOff } = shield.parts;
    return {
      working,
      also,
      line: (
        <WorkspaceOff
          plan={plan}
          words={stepWords(lang, shield.parts.pw)}
          failed={shield.parts.failedWords(lang, shield.state)}
          onRetry={
            shield.parts.needsInstaller(shield.parts.computerCause(shield.state.view))
              ? undefined
              : acts.retry
          }
          onSettings={acts.settings}
        />
      ),
    };
  }
  if (typeof signedIn === "object" && signedIn.signed_in !== undefined && signedHere)
    return {
      working,
      also,
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
  return working === null ? null : { working, also };
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
  // UC: the window's protection, where the transport carries its wire; null keeps the first run as before UC
  const [shields] = useState(() =>
    carriesProtection(transport) ? protectionStoreOf(transport) : null,
  );
  // the step's parts ride in the note's chunk, loaded once where the wire is
  const [parts, setParts] = useState<StepParts | null>(null);
  useEffect(() => {
    if (shields === null) return;
    let live = true;
    loadProtectionNote().then(
      (m) => {
        if (live) setParts(m);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [shields]);
  const readShield = () => shields?.state() ?? null;
  const shieldState = useSyncExternalStore(
    shields?.subscribe ?? noSubscribe,
    readShield,
    readShield,
  );
  const display = useDisplay();
  const { lang, platform, releaseMode } = display;
  // the plan the send uses: the open conversation's thread's, or the one a draft would run (room.tsx's openAgent)
  const openAgent = talks.rows.find((r) => r.key === talks.open)?.agent ?? null;
  const agent = openAgent ?? talks.agent;
  const fixed = openAgent !== null;
  const drafting = talks.drafting;
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
  // a plan signed in in another window (Settings' Plans) while this one's draft would run a plan that isn't: before
  // the lawyer picks here, the draft follows the one plan signed in, as `chosen` does (D-012). Only for a sign-in this
  // window heard happen (not the first word at start, which the room's default plan decides), and once per plan, so
  // the lawyer's own pick in the plan switch stands. Choosing sends nothing.
  const target = state.chosen;
  const follows =
    talks.drafting &&
    !state.picked &&
    target !== agent &&
    isSignedIn(state.agents[target]?.signIn) &&
    state.agents[target]?.signedHere === true &&
    !isSignedIn(state.agents[agent]?.signIn);
  const followed = useRef<AgentId | null>(null);
  useEffect(() => {
    if (!follows || followed.current === target) return;
    followed.current = target;
    conversations.choose(target);
  }, [follows, target, conversations]);
  // the composer reads its hold from the setup store's state as it changes (one commit per event, with this one),
  // and again when the room's side changes
  useEffect(() => {
    const holder = holderOf(transport);
    const room: RoomSide = { agent, fixed, drafting, zone, lang, platform, releaseMode };
    let cache: [SetupState, ProtectionState | null, Hold | null] | null = null;
    const withParts = (shield: ProtectionState | null) =>
      shields === null || shield === null || parts === null
        ? null
        : { store: shields, state: shield, parts };
    holder.read = () => {
      const now = store.state();
      const shield = shields?.state() ?? null;
      if (cache === null || cache[0] !== now || cache[1] !== shield)
        cache = [now, shield, holdOf(store, transport, now, room, withParts(shield))];
      return cache[2];
    };
    const notify = () => {
      for (const heard of holder.heard) heard();
    };
    const stop = store.subscribe(notify);
    const stopShield = shields?.subscribe(notify);
    notify();
    return () => {
      stop();
      stopShield?.();
      holder.read = () => null;
      notify();
    };
  }, [transport, store, shields, parts, agent, fixed, drafting, zone, lang, platform, releaseMode]);
  // UC: the Safe workspace step is entered once, after the first sign-in here; where the set-up
  // asks no administrator it is sent at once, with no confirm, and the step is its wait; where the
  // repair is the installer, nothing is sent
  const chosenSetup = state.agents[state.chosen];
  const enter =
    shields !== null &&
    parts !== null &&
    stepDue(shieldState, parts) &&
    isSignedIn(chosenSetup?.signIn) &&
    chosenSetup?.signedHere === true;
  useEffect(() => {
    if (!enter || shields === null || parts === null) return;
    shields.set({ step: "ask" });
    const view = shields.state().view;
    if (!parts.needsInstaller(parts.computerCause(view)) && !parts.asksAdmin(view))
      stepActs(transport, shields, parts).setUp();
  }, [enter, shields, parts, transport]);
  // a matter made from the rail while the first screen stood is the first act too; the matter it opens is the one
  // added during the first run, whose first read starts once signed in (UD2)
  const wasWelcome = useRef(welcome);
  useEffect(() => {
    if (
      wasWelcome.current &&
      !welcome &&
      matters.view().matters.some((m) => m.kind !== "scratch")
    ) {
      store.firstAct(null);
      const open = matters.view().open;
      if (open !== null && open.kind !== "scratch") markAdded(open.matter);
    }
    wasWelcome.current = welcome;
  }, [welcome, matters, store]);
  // UD2's first read, offered, never sent by itself (D-015): no turn goes without the lawyer's press. Its button stands
  // in the new conversation of a matter added during the first run, with papers and no conversation yet, once the
  // conversation's plan is signed in (and, after UC, the workspace step has its outcome); its chunk loads only then.
  // A sign-in, "Use … instead" and a plan switch only change what the button would use.
  const agentSetup = state.agents[agent];
  const signedIn =
    isSignedIn(agentSetup?.signIn) &&
    (agentSetup?.signedHere !== true || stepAllowsRead(workspaceStep(shieldState, parts)));
  const marks = useSyncExternalStore(subscribeMarks, firstReadMarks, firstReadMarks);
  const mark = talks.matter === null ? undefined : marks[talks.matter];
  const offer =
    !welcome &&
    signedIn &&
    talks.matter !== null &&
    talks.phase === "ready" &&
    talks.drafting &&
    talks.rows.every((r) => r.thread === null) &&
    mark?.added === true &&
    mark.thread === undefined;
  // placed by the room under its new-conversation line, as the action after that sentence
  const offerOn = offer && talks.matter !== null ? talks.matter : null;
  useEffect(() => {
    provideOffer(
      transport,
      offerOn === null ? null : (
        <Suspense fallback={null}>
          <FirstReadOffer
            transport={transport}
            matters={matters}
            conversations={conversations}
            matter={offerOn}
            agent={agent}
            title={FIRST_READ_TITLE_EN}
          />
        </Suspense>
      ),
    );
  }, [offerOn, transport, matters, conversations, agent]);
  useEffect(() => () => provideOffer(transport, null), [transport]);
  if (!welcome) return <CaseLawSlot />;
  // no gate on the first screen: each plan's line offers its own sign-in until it is signed in
  return <FirstScreen store={store} matters={matters} transport={transport} state={state} />;
}
