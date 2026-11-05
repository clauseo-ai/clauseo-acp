// Signing in, in the composer's well where the question will go (design/03-system/src/pages/first-launch/gate.tsx):
// the one act left before the first question. Two rows on the well: the act, with its words and its button where Ask
// will stand, and beneath it the quiet ways round it. It covers the plan, a login already on this computer (said before
// it is used), signing in in the browser with each way that can end, ChatGPT's device code (never the first offer:
// only after the browser didn't work), and an API key. No model is named and no lab's logo shows.
import { useEffect, useRef, type ReactNode } from "react";
import { IconOpen, IconTick } from "../../icons/icons.tsx";
import { cn } from "../../lib/cn.ts";
import { useDisplay } from "../../lib/display.ts";
import { errorWords } from "../../lib/words-errors.ts";
import { fl, type FirstLaunchKey } from "../../lib/words-first-launch.ts";
import { ui } from "../../lib/words-ui.ts";
import { Button } from "../../ui/button.tsx";
import { Say } from "../say.tsx";
import { WRAP } from "./classes.ts";
import { Act, Note, PasteRow, Ways } from "./parts.tsx";

/** A login seen but not reusable, or why reuse stopped: the host's `FoundLogin`. */
export type Found =
  | "claude_settings"
  | "claude_windows_sandbox"
  | "codex_keyring"
  | "reused_failed"
  | "reused_signed_out"
  | "reused_changed"
  | "codex_link_split";

/** Which plan: Claude's (Claude Code underneath) or ChatGPT's (Codex). */
export type Plan = "claude" | "chatgpt";

/** Where the sign-in is, as the gate shows it. The app reads it from the host's `SignIn`; `key` and `pasting` are the
 * gate's own, the panel and the paste field the lawyer opened. Times are already clock words in the language. */
export type GateView =
  | { kind: "signed-out"; found?: Found | undefined }
  | { kind: "queued"; offline: boolean }
  | { kind: "waiting"; nudge: boolean; pasting: boolean }
  | { kind: "device"; code: string; until: string }
  | { kind: "login-found"; email?: string | undefined }
  | { kind: "failed"; code: string; at: string }
  | { kind: "key" };

/** What a press asks for; the app turns each into the host's command, or the gate's own state. */
export type GateAct =
  | { act: "sign-in"; console?: boolean }
  | { act: "cancel" }
  | { act: "paste" }
  | { act: "code"; code: string }
  | { act: "device" }
  | { act: "open-page" }
  | { act: "copy-code" }
  | { act: "use-found" }
  | { act: "different" }
  | { act: "instead" }
  | { act: "key-panel"; open: boolean }
  | { act: "key"; key: string }
  | { act: "plans" };

export type GateTestIds = {
  root?: string;
  state?: string;
  cancel?: string;
  paste?: string;
  code?: string;
  deviceCode?: string;
  key?: string;
  loginFound?: string;
  loginUse?: string;
  loginOther?: string;
};

const NAME: Record<Plan, string> = { claude: "Claude", chatgpt: "ChatGPT" };
const APP: Record<Plan, string> = { claude: "Claude Code", chatgpt: "Codex" };
const other = (p: Plan): Plan => (p === "claude" ? "chatgpt" : "claude");

/** The line for a login that can't be shared, or reuse that stopped. */
const FOUND: Record<Found, FirstLaunchKey> = {
  claude_settings:
    "Claude Code on this computer signs in through your own Claude settings, which Clauseo keeps out of your matters.",
  codex_keyring: "Codex on this computer keeps its sign-in where Clauseo can't share it.",
  claude_windows_sandbox:
    "Clauseo's safe workspace can't share Claude Code's sign-in on Windows yet.",
  reused_signed_out: "{app} on this computer is signed out, so Clauseo stopped using its sign-in.",
  reused_changed:
    "{app} on this computer now uses another account, so Clauseo stopped using its sign-in.",
  codex_link_split: "Codex's sign-in on this computer changed, so Clauseo stopped using it.",
  reused_failed: "{app}'s sign-in stopped working here, so Clauseo stopped using it.",
};

/** The gate, in the field's row, for every state before the field takes a question. */
export function Gate({
  plan,
  view,
  keys,
  onAct,
  testIds = {},
}: {
  plan: Plan;
  view: GateView;
  /** Whether this computer can keep an API key, and where (display_info's platform): `null` when it can't. */
  keys: "mac" | "windows" | "linux" | null;
  onAct: (act: GateAct) => void;
  testIds?: GateTestIds;
}) {
  const { lang } = useDisplay();
  const w = (key: FirstLaunchKey, slots?: Record<string, string | number>) => fl(lang, key, slots);
  const name = NAME[plan];
  const app = APP[plan];
  const act = (a: GateAct) => () => onAct(a);
  // the key panel and the paste field take the focus as they open, and give it back to what opened them as they close
  // (Cancel goes with them); a change the host makes while the focus is elsewhere moves nothing
  const root = useRef<HTMLDivElement>(null);
  const panel =
    view.kind === "key" ? "key" : view.kind === "waiting" && view.pasting ? "paste" : null;
  const was = useRef(panel);
  useEffect(() => {
    const box = root.current;
    const before = was.current;
    was.current = panel;
    if (box === null || before === panel) return;
    if (panel !== null) {
      box.querySelector<HTMLElement>("input")?.focus();
      return;
    }
    const at = box.ownerDocument.activeElement;
    if (at !== null && at !== box.ownerDocument.body && !box.contains(at)) return;
    const buttons = [...box.querySelectorAll<HTMLButtonElement>("button")];
    (buttons.find((b) => b.dataset["opens"] === before) ?? buttons[0])?.focus();
  }, [panel]);
  const signIn = (label: string) => (
    <Button kind="tone" className="room-field-ask" onClick={act({ act: "sign-in" })}>
      {label}
    </Button>
  );
  const cancel = (
    <Button
      kind="quiet"
      className="room-field-ask fl-gate-cancel"
      onClick={act({ act: "cancel" })}
      data-testid={testIds.cancel}
    >
      {ui(lang, "Cancel")}
    </Button>
  );
  const way = (label: string, a: GateAct, testId?: string, quiet = false) => (
    <Button
      kind="quiet"
      size="small"
      // the words wrap on a narrow well: utilities, as the button's own nowrap and height are; and the inset is the
      // message's own (px-s2), where the small button's px-s3 would stand 4 px right of the words above it
      className={cn(WRAP, "min-h-control-small px-s2", quiet ? "fl-way fl-way-quiet" : "fl-way")}
      onClick={act(a)}
      data-testid={testId}
      data-opens={a.act === "key-panel" && a.open ? "key" : a.act === "paste" ? "paste" : undefined}
    >
      {label}
    </Button>
  );
  const instead = (key: boolean) => (
    <Ways>
      {way(w("Use {name} instead", { name: NAME[other(plan)] }), { act: "instead" })}
      {key &&
        way(
          w("Use an API key or my firm's account"),
          { act: "key-panel", open: true },
          undefined,
          true,
        )}
    </Ways>
  );
  let body: ReactNode;
  switch (view.kind) {
    case "signed-out":
      body = (
        <>
          <Act act={signIn(w("Sign in"))}>
            <Say>
              {view.found === undefined
                ? w("Sign in to {name} to ask about this matter", { name })
                : w(FOUND[view.found], { app })}
            </Say>
          </Act>
          {/* a login that can't be shared offers Clauseo's own sign-in, and for Claude a key or the firm's account */}
          {view.found === undefined
            ? instead(true)
            : plan === "claude" && (
                <Ways>
                  {way(
                    w("Use an API key or my firm's account"),
                    { act: "key-panel", open: true },
                    undefined,
                    true,
                  )}
                </Ways>
              )}
        </>
      );
      break;
    case "queued":
      body = (
        <Act act={cancel}>
          <Say>
            {view.offline
              ? w("Your browser opens when you reconnect.")
              : w("Your browser opens when {name} is ready.", { name })}
          </Say>
        </Act>
      );
      break;
    case "waiting":
      body = view.pasting ? (
        <>
          <PasteRow
            label={w("Code from your browser")}
            secret={false}
            go={w("Send")}
            onGo={(code) => onAct({ act: "code", code })}
            cancel={cancel}
            testId={testIds.code}
          />
          <Ways>
            <Note>{w("Clauseo passes the code to {name} and keeps no copy.", { name })}</Note>
          </Ways>
        </>
      ) : (
        <>
          <Act dot act={cancel}>
            <Say>{w("Finish signing in in your browser")}</Say>
          </Act>
          {/* only once the host says the page may not have opened (30 s): never on a timer of the window's own */}
          {view.nudge && (
            <Ways>
              <span className="fl-gate-pair">
                <Note>{w("Didn't open?")}</Note>
                {way(w("Try again"), { act: "sign-in" })}
              </span>
              {plan === "claude" ? (
                <span className="fl-gate-pair">
                  <Note>{w("Got a code instead?")}</Note>
                  {way(w("Paste it"), { act: "paste" }, testIds.paste)}
                </span>
              ) : (
                way(w("Use a code instead"), { act: "device" })
              )}
            </Ways>
          )}
        </>
      );
      break;
    case "device":
      body = (
        <>
          <Act act={cancel}>
            <span className="fl-device">
              <span className="fl-device-code" lang="en" data-testid={testIds.deviceCode}>
                {view.code}
              </span>
              <span className="fl-gate-why">
                <Say>{w("Sign in to ChatGPT on that page and enter this code.")}</Say>{" "}
                <Say>{w("The code works until {time}.", { time: view.until })}</Say>
              </span>
            </span>
          </Act>
          <Ways>
            {way(w("Open page"), { act: "open-page" })}
            {way(w("Copy"), { act: "copy-code" })}
          </Ways>
        </>
      );
      break;
    case "login-found":
      body = (
        <>
          <Act
            act={
              <Button
                kind="tone"
                className="room-field-ask"
                onClick={act({ act: "use-found" })}
                data-testid={testIds.loginUse}
              >
                {w("Use it")}
              </Button>
            }
          >
            <span data-testid={testIds.loginFound}>
              <Say>
                {view.email === undefined
                  ? w("Clauseo found your {name} account from {app} and will use it here.", {
                      name,
                      app,
                    })
                  : w(
                      "Clauseo found your {name} account ({email}) from {app} and will use it here.",
                      {
                        name,
                        app,
                        email: view.email,
                      },
                    )}
              </Say>
            </span>
          </Act>
          <Ways>
            {way(w("Sign in with a different account"), { act: "different" }, testIds.loginOther)}
            <Note>
              {w("A different account signs in to Clauseo only. {app} stays as it is.", { app })}
            </Note>
          </Ways>
        </>
      );
      break;
    case "key":
      body = (
        <>
          {keys === null ? (
            <Act
              act={
                <Button
                  kind="quiet"
                  className="room-field-ask fl-gate-cancel"
                  onClick={act({ act: "key-panel", open: false })}
                >
                  {ui(lang, "Cancel")}
                </Button>
              }
            >
              <Say>{w("This computer has no keyring to keep an API key in.")}</Say>
            </Act>
          ) : (
            <PasteRow
              label={w("API key")}
              secret
              go={w("Use this key")}
              onGo={(key) => onAct({ act: "key", key })}
              testId={testIds.key}
              cancel={
                <Button
                  kind="quiet"
                  className="fl-gate-cancel"
                  onClick={act({ act: "key-panel", open: false })}
                >
                  {ui(lang, "Cancel")}
                </Button>
              }
            />
          )}
          <Ways>
            {keys !== null && (
              <Note>
                {`${w("A key from Claude or ChatGPT, or one from your firm's IT team. Billed to the key's own account.")} ${w(
                  keys === "mac"
                    ? "Kept in this Mac's Keychain."
                    : keys === "windows"
                      ? "Kept in Windows Credential Manager."
                      : "Kept in this computer's keyring.",
                )}`}
              </Note>
            )}
            {plan === "claude" &&
              way(w("Sign in with an Anthropic Console account"), {
                act: "sign-in",
                console: true,
              })}
          </Ways>
        </>
      );
      break;
    case "failed":
      body = (
        <Failed
          plan={plan}
          view={view}
          w={w}
          way={way}
          signIn={signIn}
          instead={instead}
          onAct={onAct}
        />
      );
      break;
  }
  return (
    <div ref={root} className="fl-gate" data-testid={testIds.root} data-agent={plan}>
      <div className="fl-gate-body" data-testid={testIds.state} data-state={stateOf(view)}>
        {body}
      </div>
    </div>
  );
}

/** The state the gate is in, for its test id's data-state: the view's kind, a failure's code. */
const stateOf = (view: GateView): string => (view.kind === "failed" ? view.code : view.kind);

/** A sign-in that stopped, by its code. */
function Failed({
  plan,
  view,
  w,
  way,
  signIn,
  instead,
  onAct,
}: {
  plan: Plan;
  view: { code: string; at: string };
  w: (key: FirstLaunchKey, slots?: Record<string, string | number>) => string;
  way: (label: string, a: GateAct) => ReactNode;
  signIn: (label: string) => ReactNode;
  instead: (key: boolean) => ReactNode;
  onAct: (act: GateAct) => void;
}) {
  const { lang } = useDisplay();
  const name = NAME[plan];
  const plans = () => onAct({ act: "plans" });
  switch (view.code) {
    case "signin.cancelled":
      return (
        <>
          <Act act={signIn(w("Sign in"))}>
            <Say>{errorWords(lang, "signin.cancelled")}</Say>
          </Act>
          {instead(true)}
        </>
      );
    case "signin.link_expired":
      return (
        <>
          <Act act={signIn(w("Try again"))}>
            <Say>{w("The sign-in link expired at {time}.", { time: view.at })}</Say>
          </Act>
          {instead(true)}
        </>
      );
    case "signin.no_plan":
      return (
        <>
          <Act
            act={
              <Button kind="tone" className="room-field-ask" onClick={plans}>
                {w("See {name}'s plans", { name })}
                <IconOpen size={14} />
              </Button>
            }
          >
            <Say>{errorWords(lang, "signin.no_plan")}</Say>{" "}
            <span className="fl-gate-why">
              <Say>
                {plan === "claude"
                  ? w("Clauseo works with a Claude Pro or Max plan.")
                  : w("Clauseo works with a paid ChatGPT plan.")}
              </Say>
            </span>
          </Act>
          <Ways>
            {way(w("Use another account"), { act: "sign-in" })}
            {way(w("Use {name} instead", { name: NAME[other(plan)] }), { act: "instead" })}
          </Ways>
        </>
      );
    case "signin.device_code_off":
      return (
        <>
          <Act
            act={
              <Button
                kind="tone"
                className="room-field-ask"
                onClick={() => onAct({ act: "device" })}
              >
                {w("Try again")}
              </Button>
            }
          >
            <Say>{errorWords(lang, "signin.device_code_off")}</Say>{" "}
            <span className="fl-gate-why">
              <Say>
                {w(
                  "To turn it on, open ChatGPT, then Settings, then Security, and turn on Enable device code sign-in for Codex, Excel, PowerPoint, and Word. Then choose Try again.",
                )}
              </Say>
            </span>
          </Act>
          <Ways>{way(w("Sign in in the browser instead"), { act: "sign-in" })}</Ways>
        </>
      );
    case "signin.port_busy":
      return (
        <>
          <Act act={signIn(w("Try again"))}>
            <Say>{errorWords(lang, "signin.port_busy")}</Say>
          </Act>
          <Ways>{way(w("Use a code instead"), { act: "device" })}</Ways>
        </>
      );
    case "signin.policy_unreadable":
      return (
        <Act>
          <Say>
            {w(
              "Clauseo can't read your firm's settings for {name}, so it won't sign in until they can be read.",
              { name },
            )}
          </Say>{" "}
          <span className="fl-gate-why">
            <Say>{w("Ask your IT team.")}</Say>
          </span>
        </Act>
      );
    default:
      return (
        <Act act={signIn(w("Try again"))}>
          <Say>{w("Sign-in didn't finish.")}</Say>
        </Act>
      );
  }
}

/** The signed-in line: the account and plan when the host has them, how when it is a key or the firm's, and through
 * which of the lawyer's own tools when Clauseo reuses that login. It stands inside the room's line. */
export function SignedLine({
  plan,
  method,
  reused,
  email,
  label,
  testId,
}: {
  plan: Plan;
  method: "plan" | "api_key" | "firm";
  reused: boolean;
  email?: string | undefined;
  label?: string | undefined;
  testId?: string | undefined;
}) {
  const { lang } = useDisplay();
  const w = (key: FirstLaunchKey, slots?: Record<string, string | number>) => fl(lang, key, slots);
  const app = APP[plan];
  const words =
    method === "api_key"
      ? w("Using an API key, billed to its own account")
      : method === "firm"
        ? w("Using your firm's account")
        : reused
          ? email !== undefined && label !== undefined
            ? w("Signed in through {app} as {email} · {plan}", { app, email, plan: label })
            : email !== undefined
              ? w("Signed in through {app} as {email}", { app, email })
              : w("Signed in through {app}", { app })
          : email !== undefined && label !== undefined
            ? w("Signed in as {email} · {plan}", { email, plan: label })
            : email !== undefined
              ? w("Signed in as {email}", { email })
              : w("Signed in to {name}", { name: NAME[plan] });
  // on the room's line under the field, beside its other lines: the tick in primary, never green
  return (
    <>
      <IconTick size={14} className="flex-none fl-done-tick" />
      <span role="status" data-testid={testId}>
        <Say>{words}</Say>
      </span>
    </>
  );
}
