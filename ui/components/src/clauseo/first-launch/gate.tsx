// Signing in, in the composer's well where the question will go (design/03-system/src/pages/first-launch/gate.tsx):
// the one act left before the first question. Two rows on the well: the act, with its words and its button where Ask
// will stand, and beneath it the quiet ways round it. It covers the plan, a login already on this computer (said before
// it is used), signing in in the browser with each way that can end, ChatGPT's device code (never the first offer:
// only after the browser didn't work), and an API key. No model is named and no lab's logo shows.
import { useEffect, useRef, type ReactNode } from "react";
import { IconTick } from "../../icons/icons.tsx";
import { cn } from "../../lib/cn.ts";
import { useDisplay } from "../../lib/display.ts";
import { errorWords } from "../../lib/words-errors.ts";
import { fl, type FirstLaunchKey } from "../../lib/words-first-launch.ts";
import { ui } from "../../lib/words-ui.ts";
import { Button } from "../../ui/button.tsx";
import { Say } from "../say.tsx";
import { WRAP } from "./classes.ts";
import { Failed } from "./gate-failed.tsx";
import { Act, Note, PasteRow, Ways } from "./parts.tsx";
import { NAME, other } from "./plan-names.ts";

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
  | { kind: "switching"; stuck: boolean }
  | { kind: "key" };

/** A setup command desktop refused, for the line under the gate: its code, and whether the agent is switching
 * accounts, when a `command.conflict` says only that the switch isn't done. */
export type GateRefusal = { code: string; switching: boolean };

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

const APP: Record<Plan, string> = { claude: "Claude Code", chatgpt: "Codex" };

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
  refused,
  testIds = {},
}: {
  plan: Plan;
  view: GateView;
  /** The last setup command desktop refused for this plan, until its next sign-in change or press. */
  refused?: GateRefusal | null | undefined;
  /** Whether this computer can keep an API key, and where (display_info's platform): `null` when it can't. */
  keys: "mac" | "windows" | "linux" | null;
  /** A press. For a code or a key, the promise of whether desktop took it: its field keeps the words until then, and
   * after a refusal. */
  onAct: (act: GateAct) => void | Promise<boolean>;
  testIds?: GateTestIds;
}) {
  const { lang } = useDisplay();
  const w = (key: FirstLaunchKey, slots?: Record<string, string | number>) => fl(lang, key, slots);
  const name = NAME[plan];
  const app = APP[plan];
  const act = (a: GateAct) => () => void onAct(a);
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
    case "switching":
      // Clauseo is leaving a login it may no longer use and retries by itself: nothing to press, and no sign-in offered
      body = (
        <Act>
          <Say>{w("Switching accounts.")}</Say>{" "}
          <span className="fl-gate-why">
            <Say>
              {w(
                "Clauseo is still stopping the old login. It keeps trying by itself, so there's nothing you need to do.",
              )}
            </Say>
            {view.stuck && (
              <>
                {" "}
                <Say>{w("If this goes on, quit Clauseo and open it again.")}</Say>
              </>
            )}
          </span>
        </Act>
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
        {refused != null && (
          <Ways>
            <span className="fl-gate-note" role="status" data-testid="signin-refused">
              <Say>
                {refused.switching && refused.code === "command.conflict"
                  ? w("Clauseo is still switching accounts. Try again in a moment.")
                  : errorWords(lang, refused.code)}
              </Say>
            </span>
          </Ways>
        )}
      </div>
    </div>
  );
}

/** The state the gate is in, for its test id's data-state: the view's kind, a failure's code. */
const stateOf = (view: GateView): string => (view.kind === "failed" ? view.code : view.kind);

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
