// One plan's block in Plans (UD2): its name, then where its sign-in stands, in UD1's words and parts, and what this
// window can do about it. Signed in: UD1's signed-in line, with "Sign out" (an own plan login) or "Stop using it here"
// (a reused one), each asking first; with a key or the firm's account, the line alone, since the host bars a sign-in
// then. Signed out: "Sign in again" and "Look for my … sign-in", or UD1's line for a login found but not reusable.
// Signing in: UD1's gate, inline. ChatGPT not added: "Add ChatGPT", which installs it and then starts its sign-in.
// Under it, the other ways in (a key; for Claude the firm's address). A plan the firm's policy locks shows its line
// and "Managed by firm", and no control.
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
import { Button } from "@clauseo/components/ui/button.tsx";
import { clockOf, localZone } from "../../room/when.ts";
import { gateView, installView, refusalView } from "../../room/first-run/views.ts";
import { CODEX, isReady, type AgentSetup, type SetupStore } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId, FoundLogin, SignIn } from "../../transport/protocol.ts";
import { Managed } from "./parts.tsx";
import { PlansEndpoint } from "./plans-endpoint.tsx";
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

type SignedIn = NonNullable<Extract<SignIn, { signed_in: unknown }>["signed_in"]>;

/** Where the block stands. */
type Stand =
  | { kind: "none" }
  | { kind: "add"; mb: number }
  | { kind: "install" }
  | { kind: "signed"; signed: SignedIn }
  | { kind: "signed-out"; found: PlansFound | undefined }
  /** A stored key that couldn't be read in time (agent.key_unreadable, #506): its words, and a sign-in, which the
   * host doesn't bar meanwhile. */
  | { kind: "failed"; code: string }
  | { kind: "gate" };

function standOf(agent: AgentId, setup: AgentSetup | undefined, adding: boolean): Stand {
  const install = setup?.install;
  if (install !== undefined && install !== "managed" && !isReady(install)) {
    if (agent === CODEX && !adding && typeof install === "object" && install.missing !== undefined)
      return { kind: "add", mb: setup?.mb ?? 0 };
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

/** Adds waiting for their install, by store: "Add ChatGPT" starts the sign-in once the install is ready, though the
 * section was closed and opened meanwhile. */
const adds = new WeakMap<SetupStore, Set<AgentId>>();
const addsOf = (store: SetupStore) => {
  let set = adds.get(store);
  if (set === undefined) adds.set(store, (set = new Set()));
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
  onOther: () => void;
  blockRef: (el: HTMLElement | null) => void;
}) {
  const { lang } = useDisplay();
  const s = (key: SettingsKey, slots?: Record<string, string | number>) => st(lang, key, slots);
  const plan: Plan = agent === CODEX ? "chatgpt" : "claude";
  const name = NAME[plan];
  const app = APP[plan];
  const [pasting, setPasting] = useState(false);
  const [signedOutHere, setSignedOutHere] = useState(false);
  // "Add ChatGPT" pressed: the add waits in the store's set until the install is ready (the state only re-renders)
  const [, setAdding] = useState(false);
  const adding = addsOf(store).has(agent);
  const stand = standOf(agent, setup, adding);
  const ready = isReady(setup?.install);
  // the install "Add ChatGPT" started is ready: its sign-in starts, once
  useEffect(() => {
    if (!ready || !addsOf(store).delete(agent)) return;
    store.command({ start_sign_in: { agent } });
  }, [ready, store, agent]);
  const command = store.command;
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
      case "key-panel":
        // the key's field stands under the block already: the press takes the lawyer there
        if (a.open)
          document
            .querySelector<HTMLInputElement>(`[data-agent="${plan}"] [data-testid="plans-key"]`)
            ?.focus();
        return;
      case "key":
        return store.send({ set_agent_key: { agent, key: a.key } });
      case "plans":
        transport
          .openExternal(PRICES[plan])
          .catch((error: unknown) =>
            console.warn(`open_external failed: ${JSON.stringify(error)}`),
          );
        return;
    }
  };
  const signedLine = (signed: SignedIn) => (
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
          <>
            {line(s("Uses your ChatGPT plan."))}
            <p className="set-plan-fine">
              {say(
                s(
                  "Only ChatGPT's part downloads, about {mb} MB. Signing in happens in your browser.",
                  { mb: stand.mb },
                ),
              )}
            </p>
            <div className="set-plan-acts">
              <Button
                kind="tone"
                size="small"
                data-testid="plans-add"
                onClick={() => {
                  // desktop's own confirm names the size; a declined one leaves the block as it was
                  addsOf(store).add(agent);
                  setAdding(true);
                  void store.send({ install_agent: { agent } }).then((ok) => {
                    if (ok) return;
                    addsOf(store).delete(agent);
                    setAdding(false);
                  });
                }}
              >
                {say(s("Add ChatGPT"))}
              </Button>
            </div>
          </>
        );
        break;
      case "install": {
        const working = installLine(lang, installView(setup), name, {
          retry: () => command({ retry_agent_install: { agent } }),
          proxy: () => command({ retry_agent_install: { agent, use_system_proxy: true } }),
          copy: copyText,
          copied: null,
          setCopied: () => {},
        });
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
          <>
            {line(fl(lang, FOUND[found as Found], { app }))}
            <div className="set-plan-acts">
              <Button kind="tone" size="small" onClick={signIn}>
                {say(fl(lang, "Sign in"))}
              </Button>
              {found === "reused_failed" && (
                <Button
                  kind="quiet"
                  size="small"
                  onClick={() => command({ use_different_account: { agent } })}
                >
                  {say(fl(lang, "Sign in with a different account"))}
                </Button>
              )}
            </div>
          </>
        ) : (
          <>
            {signedOutHere && found === undefined && line(s("Signed out of {name}.", { name }))}
            {found === "none_found" && line(s("No {app} sign-in on this computer.", { app }))}
            {found === "reuse_off" && line(s("Clauseo isn't using {app}'s sign-in here.", { app }))}
            <div className="set-plan-acts">
              <Button kind="tone" size="small" onClick={signIn}>
                {say(s("Sign in again"))}
              </Button>
              <Button
                kind="quiet"
                size="small"
                data-testid="plans-find-login"
                onClick={() => command({ find_login: { agent } })}
              >
                {say(s("Look for my {app} sign-in", { app }))}
              </Button>
            </div>
          </>
        );
        break;
      }
      case "failed":
        body = (
          <>
            {line(errorWords(lang, stand.code))}
            <div className="set-plan-acts">
              <Button kind="tone" size="small" onClick={signIn}>
                {say(s("Sign in again"))}
              </Button>
            </div>
          </>
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
        body = view !== null && (
          <div className="set-plan-gate">
            <Gate plan={plan} view={view} keys={keys} onAct={onAct} />
          </div>
        );
        break;
      }
    }
  }
  // the other ways in, once the plan is here and the firm's policy leaves them to the lawyer
  const ways = !locked && stand.kind !== "none" && stand.kind !== "add" && stand.kind !== "install";
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
        <section className="set-plan-ways" aria-label={s("Other ways in")}>
          <Heading level={4} className="set-plan-sub">
            {say(s("Other ways in"))}
          </Heading>
          <PlansKey
            keys={keys}
            keysKnown={keysKnown}
            inUse={method === "api_key"}
            onKey={(key) => store.send({ set_agent_key: { agent, key } })}
            onRemove={() => command({ clear_agent_key: { agent } })}
          />
          {plan === "claude" && (
            <PlansEndpoint
              key={endpoint ?? ""}
              endpoint={endpoint}
              onConnect={(url) =>
                command({ set_firm_endpoint: url === undefined ? { agent } : { agent, url } })
              }
            />
          )}
        </section>
      )}
    </article>
  );
}
