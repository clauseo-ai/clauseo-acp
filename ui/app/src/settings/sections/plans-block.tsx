// One plan's block in Plans (UD2): its name, then where its sign-in stands, in UD1's words and parts, and what this
// window can do about it. Signed in: UD1's signed-in line, with "Sign out" (an own plan login) or "Stop using it here"
// (a reused one), each asking first; with a key or the firm's account, the line alone, since the host bars a sign-in
// then. Signed out: "Sign in again" and "Look for my … sign-in", or UD1's line for a login found but not reusable.
// Signing in: UD1's gate, inline. A plan not added yet, either one: "Add Claude" or "Add ChatGPT", which installs it
// and then starts its sign-in. Once its install runs: the install line. Every plan not in use yet (not added, waiting
// for its download, signed out, a sign-in that stopped) is the one card, plans-card.tsx, the same for both plans.
// Under it, one quiet "Use an API key instead", which opens the key's panel (plans-key.tsx), whether or not the plan is
// added yet: a key saved for a plan not here starts its download. A key the provider refused says so in the line. A plan the firm's policy
// locks shows its line (and for Claude the firm's address its policy set) and "Managed by firm", and no control. The
// firm's address is set by the firm's policy only (plans.<id>, docs/firm-it.md): Plans offers no field for it.
import { useEffect, useState, type ReactNode } from "react";
import {
  Gate,
  SignedLine,
  type Found,
  type GateAct,
  type Plan,
} from "@clauseo/components/clauseo/first-launch/gate.tsx";
import { installLine } from "@clauseo/components/clauseo/first-launch/install.tsx";
import { APP, FOUND, NAME } from "@clauseo/components/clauseo/first-launch/plan-names.ts";
import { Heading } from "@clauseo/components/clauseo/heading.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { st, type SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { clockOf, localZone } from "../../room/when.ts";
import { aboutMb, gateView, installView, refusalView } from "../../room/first-run/views.ts";
import {
  CODEX,
  installForKey,
  isReady,
  keyRefused,
  type AgentSetup,
  type SetupStore,
} from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId, FoundLogin, SignIn } from "../../transport/protocol.ts";
import { Managed } from "./parts.tsx";
import { MainAct, OtherAct, PlanCard, Stopped } from "./plans-card.tsx";
import { PlansKey } from "./plans-key.tsx";
import { Confirm, Refused } from "./plans-parts.tsx";

/** What a search for the CLI's login can answer, read so both wires compile: today's FoundLogin, and stage D's with
 * `none_found` (the search found nothing) and `reuse_off` (reuse is off here, as after "Stop using it here"). */
type PlansFound = FoundLogin | "none_found" | "reuse_off";

/** Each plan's prices page, for an account with no plan (H11's open_external asks first). */
const PRICES: Record<Plan, string> = {
  claude: "https://claude.com/pricing",
  chatgpt: "https://chatgpt.com/pricing",
};

/** The sign-in codes Plans says in its own card (plans-card.tsx's Stopped) rather than UD1's gate. */
const STOPPED = new Set(["signin.cancelled", "signin.link_expired", "signin.no_plan"]);
const isStopped = (code: string) => STOPPED.has(code);

type SignedIn = NonNullable<Extract<SignIn, { signed_in: unknown }>["signed_in"]>;

/** Where the block stands. */
type Stand =
  | { kind: "none" }
  /** Not here yet; `paused` when the lawyer paused its download (the other plan's never pauses it). */
  | { kind: "add"; mb: number; paused?: true }
  | { kind: "install" }
  | { kind: "signed"; signed: SignedIn }
  | { kind: "signed-out"; found: PlansFound | undefined }
  /** A stored key that couldn't be read in time (agent.key_unreadable, #506): its words, and a sign-in, which the
   * host doesn't bar meanwhile. */
  | { kind: "failed"; code: string }
  | { kind: "gate" };

function standOf(setup: AgentSetup | undefined, adding: boolean): Stand {
  const install = setup?.install;
  if (install !== undefined && install !== "managed" && !isReady(install)) {
    if (!adding && typeof install === "object" && install.missing !== undefined)
      return { kind: "add", mb: aboutMb(setup?.mb ?? 0) };
    // its download paused by the lawyer: the same card, whose press queues the sign-in; once queued, the gate
    if (!adding && install === "paused")
      return setup?.signIn === "queued"
        ? { kind: "gate" }
        : { kind: "add", mb: aboutMb(setup?.mb ?? 0), paused: true };
    if (installView(setup) !== null) return { kind: "install" };
  }
  const signIn = setup?.signIn;
  if (signIn === undefined) return { kind: "none" };
  if (typeof signIn === "object") {
    if (signIn.signed_in !== undefined) return { kind: "signed", signed: signIn.signed_in };
    if (signIn.signed_out !== undefined)
      return { kind: "signed-out", found: signIn.signed_out.found as PlansFound | undefined };
    // the agent's own refusal for no sign-in is the signed-out block again, as the gate has it
    if (signIn.failed?.code === "agent.auth_required")
      return { kind: "signed-out", found: undefined };
    if (signIn.failed?.code === "agent.key_unreadable")
      return { kind: "failed", code: signIn.failed.code };
  }
  return { kind: "gate" };
}

/** Adds waiting for their install, by store: "Add {plan}" starts the sign-in once the install is ready, though the
 * section was closed and opened meanwhile. */
const adds = new WeakMap<SetupStore, Set<AgentId>>();
const addsOf = (store: SetupStore) => {
  let set = adds.get(store);
  if (set === undefined) adds.set(store, (set = new Set()));
  return set;
};

/** Plans this window has seen signed in, by store, though the section was closed and opened meanwhile: after one
 * of them signs out, its card says "Sign in again". The host's SignedOut carries no word of an earlier sign-in. */
const seen = new WeakMap<SetupStore, Set<AgentId>>();
const seenOf = (store: SetupStore) => {
  let set = seen.get(store);
  if (set === undefined) seen.set(store, (set = new Set()));
  return set;
};

const copyText = (text: string) => void navigator.clipboard?.writeText(text).catch(() => {});

export function PlanBlock({
  agent,
  setup,
  keys,
  keysKnown,
  store,
  transport,
  locked,
  endpoint,
  otherReady,
  onOther,
  blockRef,
}: {
  agent: AgentId;
  setup: AgentSetup | undefined;
  keys: "mac" | "windows" | "linux" | null;
  keysKnown: boolean;
  store: SetupStore;
  transport: HostTransport;
  locked: boolean;
  endpoint: string | undefined;
  /** The other plan can be used here (installed or signed in, not held by the firm): only then "Use … instead". */
  otherReady: boolean;
  onOther: () => void;
  blockRef: (el: HTMLElement | null) => void;
}) {
  const { lang } = useDisplay();
  const s = (key: SettingsKey, slots?: Record<string, string | number>) => st(lang, key, slots);
  const plan: Plan = agent === CODEX ? "chatgpt" : "claude";
  const name = NAME[plan];
  const app = APP[plan];
  const [pasting, setPasting] = useState(false);
  const [keyOpen, setKeyOpen] = useState(false);
  const [signedOutHere, setSignedOutHere] = useState(false);
  // "Add {plan}" pressed: the add waits in the store's set until the install is ready (the state only re-renders)
  const [, setAdding] = useState(false);
  const adding = addsOf(store).has(agent);
  const stand = standOf(setup, adding);
  const ready = isReady(setup?.install);
  const signedNow = stand.kind === "signed";
  useEffect(() => {
    if (signedNow) seenOf(store).add(agent);
  }, [signedNow, store, agent]);
  // a real sign-in before this one: seen signed in here, or the agent's own refusal for a sign-in that lapsed
  // (agent.auth_required); otherwise the first sign-in's words, never "again"
  const before =
    signedOutHere ||
    seenOf(store).has(agent) ||
    (typeof setup?.signIn === "object" && setup.signIn.failed?.code === "agent.auth_required");
  // the install "Add {plan}" started is ready: its sign-in starts, once
  useEffect(() => {
    if (!ready || !addsOf(store).delete(agent)) return;
    store.command({ start_sign_in: { agent } });
  }, [ready, store, agent]);
  const command = store.command;
  // a saved key on a plan not added yet starts its download at once (installForKey), so the next question can go
  const saveKey = (key: string) =>
    store.send({ set_agent_key: { agent, key } }).then((ok) => {
      if (ok) installForKey(store, agent);
      return ok;
    });
  const signIn = () => {
    setPasting(false);
    setSignedOutHere(false);
    command({ start_sign_in: { agent } });
  };
  const onAct = (a: GateAct): void | Promise<boolean> => {
    switch (a.act) {
      case "sign-in":
        setPasting(false);
        command({ start_sign_in: a.console === true ? { agent, console: true } : { agent } });
        return;
      case "cancel":
        setPasting(false);
        command({ cancel_sign_in: { agent } });
        return;
      case "paste":
        setPasting(true);
        return;
      case "code":
        return store.send({ paste_sign_in_code: { agent, code: a.code } }).then((ok) => {
          if (ok) setPasting(false);
          return ok;
        });
      case "device":
        command({ use_device_code: { agent } });
        return;
      case "open-page":
        command({ open_sign_in_page: { agent } });
        return;
      case "copy-code": {
        const signIn = setup?.signIn;
        if (typeof signIn === "object" && signIn.device_code !== undefined)
          copyText(signIn.device_code.code);
        return;
      }
      case "use-found":
        command({ use_found_login: { agent } });
        return;
      case "different":
        command({ use_different_account: { agent } });
        return;
      case "instead":
        onOther();
        return;
      case "sign-in-to":
        // the gate's own plan signs in here; the other plan's press moves to its block, as "Use … instead" does
        if (a.plan === plan) signIn();
        else onOther();
        return;
      case "key-panel":
        // the key's panel under the block opens, its field taking the focus (plans-key.tsx)
        setKeyOpen(a.open);
        return;
      case "key":
        return saveKey(a.key);
      case "plans":
        transport
          .openExternal(PRICES[plan])
          .catch((error: unknown) =>
            console.warn(`open_external failed: ${JSON.stringify(error)}`),
          );
        return;
    }
  };
  const signedLine = (signed: SignedIn) =>
    // the provider refused the key in use (the host's refused, after a question failed): said, never "✓ Using an API
    // key"; the key's panel under the block changes or removes it
    signed.method === "api_key" && keyRefused({ signed_in: signed }) ? (
      <p className="set-plan-line" data-testid="plans-key-refused">
        {say(s("{name} didn't accept the API key.", { name }))}
      </p>
    ) : (
      <p className="set-plan-line set-plan-signed">
        <SignedLine
          plan={plan}
          method={signed.method}
          reused={signed.source === "reused"}
          email={signed.email}
          label={signed.plan}
        />
      </p>
    );
  const line = (words: string) => <p className="set-plan-line">{say(words)}</p>;
  let body: ReactNode = null;
  if (locked) {
    // the firm's policy holds the plan: its line, and no control (the host refuses a sign-in, signin.firm_managed)
    body = (
      <>
        {stand.kind === "signed" && signedLine(stand.signed)}
        {plan === "claude" && endpoint !== undefined && (
          <p className="set-plan-fine as-en" lang="en">
            {endpoint}
          </p>
        )}
        <div data-testid="plans-locked">
          <Managed id={`plans-managed-${plan}`} />
        </div>
      </>
    );
  } else {
    switch (stand.kind) {
      case "none":
        break;
      case "add":
        body = (
          <PlanCard
            plan={plan}
            // the download's size, about, then what to expect; with no size known, what to expect alone
            note={
              stand.mb > 0
                ? s("Needs a download of about {mb} MB. Signing in happens in your browser.", {
                    mb: stand.mb,
                  })
                : s("Signing in happens in your browser.")
            }
          >
            <MainAct
              label={s(plan === "claude" ? "Add Claude" : "Add ChatGPT")}
              testId="plans-add"
              onClick={() => {
                // waiting for the other download: the sign-in queues, and desktop downloads this plan next
                if (stand.paused === true) return signIn();
                // the install starts with no confirm (the card named the size); a refused send leaves it as it was
                addsOf(store).add(agent);
                setAdding(true);
                void store.send({ install_agent: { agent } }).then((ok) => {
                  if (ok) return;
                  addsOf(store).delete(agent);
                  setAdding(false);
                });
              }}
            />
          </PlanCard>
        );
        break;
      case "install": {
        const working = installLine(
          lang,
          installView(setup),
          name,
          {
            retry: () => command({ retry_agent_install: { agent } }),
            proxy: () => command({ retry_agent_install: { agent, use_system_proxy: true } }),
            copy: copyText,
            copied: null,
            setCopied: () => {},
            // a plan not here yet gets the room gate's Sign in: desktop downloads it and signs in once it is ready
            // (not while "Add {plan}" waits: its own sign-in starts at ready)
            signIn: adding ? undefined : signIn,
          },
          { signIn: "plans-sign-in" },
        );
        body = working !== null && working.kind === "wait" && (
          <div className="set-plan-install">
            {line(working.words)}
            {working.actions}
          </div>
        );
        break;
      }
      case "signed": {
        const { signed } = stand;
        body = (
          <>
            {signedLine(signed)}
            {signed.method === "plan" && (
              <div className="set-plan-acts">
                {signed.source === "reused" ? (
                  <Confirm
                    label={s("Stop using it here")}
                    title={s("Stop using {app}'s sign-in here?", { app })}
                    body={s(
                      "This doesn't sign you out of {app}. Clauseo stops using its sign-in, and you can sign in here instead.",
                      { app },
                    )}
                    onConfirm={() => command({ stop_using_login: { agent } })}
                    testId="plans-stop-using"
                  />
                ) : (
                  <Confirm
                    label={s("Sign out")}
                    title={s("Sign out of {name}?", { name })}
                    body={s(
                      "Your matters and conversations stay on this computer. Questions wait until you sign in again.",
                    )}
                    onConfirm={() => {
                      setSignedOutHere(true);
                      command({ sign_out: { agent } });
                    }}
                    testId="plans-sign-out"
                  />
                )}
              </div>
            )}
          </>
        );
        break;
      }
      case "signed-out": {
        const { found } = stand;
        const known = found !== undefined && found !== "none_found" && found !== "reuse_off";
        body = known ? (
          <PlanCard plan={plan} note={fl(lang, FOUND[found as Found], { app })}>
            <MainAct label={fl(lang, "Sign in")} onClick={signIn} testId="plans-sign-in" />
            {found === "reused_failed" && (
              <OtherAct
                label={fl(lang, "Sign in with a different account")}
                onClick={() => command({ use_different_account: { agent } })}
              />
            )}
          </PlanCard>
        ) : (
          <PlanCard
            plan={plan}
            note={
              found === "none_found"
                ? s("No {app} sign-in on this computer.", { app })
                : found === "reuse_off"
                  ? s("Clauseo isn't using {app}'s sign-in here.", { app })
                  : signedOutHere
                    ? s("Signed out of {name}.", { name })
                    : undefined
            }
          >
            <MainAct
              label={before ? s("Sign in again") : fl(lang, "Sign in")}
              onClick={signIn}
              testId="plans-sign-in"
            />
            <OtherAct
              label={s("Look for my {app} sign-in", { app })}
              onClick={() => command({ find_login: { agent } })}
              testId="plans-find-login"
            />
          </PlanCard>
        );
        break;
      }
      case "failed":
        body = (
          <PlanCard plan={plan} note={errorWords(lang, stand.code)}>
            <MainAct label={s("Sign in again")} onClick={signIn} testId="plans-sign-in" />
          </PlanCard>
        );
        break;
      case "gate": {
        const zone = localZone();
        // a clock time only, so the host's time stands for its own day
        const view = gateView(
          setup,
          { ...store.state(), keyOpen: false, pasting },
          (at) => clockOf(at, at, zone, lang).time,
        );
        // a sign-in that stopped in a way the card says: the card, with no key link (the key stands under it)
        if (view !== null && view.kind === "failed" && isStopped(view.code)) {
          body = (
            <Stopped
              plan={plan}
              lang={lang}
              code={view.code}
              at={view.at}
              otherReady={otherReady}
              onSignIn={signIn}
              onOther={onOther}
              onPlans={() => onAct({ act: "plans" })}
            />
          );
          break;
        }
        body = view !== null && (
          <div className="set-plan-gate">
            <Gate plan={plan} view={view} keys={keys} onAct={onAct} />
          </div>
        );
        break;
      }
    }
  }
  // the key whether or not the plan is added yet (saving it starts the download), where the firm's policy leaves it to
  // the lawyer
  const ways = !locked && stand.kind !== "none";
  const method = stand.kind === "signed" ? stand.signed.method : null;
  return (
    <article
      ref={blockRef}
      className="set-plan"
      aria-label={name}
      data-testid="plans-row"
      data-agent={plan}
      data-state={locked ? "locked" : stand.kind}
    >
      <Heading level={3} className="set-plan-name as-en" lang="en">
        {name}
      </Heading>
      {body}
      <Refused refused={refusalView(setup)} />
      {ways && (
        <PlansKey
          plan={plan}
          keys={keys}
          keysKnown={keysKnown}
          inUse={method === "api_key"}
          open={keyOpen}
          onOpen={setKeyOpen}
          onKey={saveKey}
          onRemove={() => command({ clear_agent_key: { agent } })}
        />
      )}
    </article>
  );
}
