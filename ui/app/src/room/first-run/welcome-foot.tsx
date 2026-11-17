// One plan's working line on the first screen, at its foot where the composer will stand (one for each plan, Claude's
// then ChatGPT's): the install's line while the plan isn't here yet (install.tsx, with "Sign in with {name}" while it is
// missing), and once it is installed but not signed in, the plan's "Sign in with {name}" in the same place (busy while the sign-in status check runs), so a lawyer can get the plan going
// without opening a matter. No gate stands on the first screen: the line says the gate's own words for a sign-in under
// way and offers its Cancel, never a second Sign in; the states only a matter's gate walks (a device code, switching
// accounts) leave the foot as it was; a login found says the gate's line with its Use it. A plan whose sign-in waits (the setup store's `queued`, behind
// the other plan's sign-in, or desktop's, behind its own download) keeps its download's line with a Cancel that stops
// the download and the wait (the plan reads not installed after), and once it is here says when its browser opens,
// with "Cancel {name}'s sign-in" for the other plan's sign-in ahead of it only while that one shows no line of its own
// (a code, a switch of accounts). The composer shows the other plan's line the same way. A plan the firm's policy locks
// shows no control. Settings' Plans has its own Sign in again.
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import {
  checkingLine,
  installLine,
  type InstallActs,
  type InstallTestIds,
} from "@clauseo/components/clauseo/first-launch/install.tsx";
import { APP } from "@clauseo/components/clauseo/first-launch/plan-names.ts";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { isKeyNotRead } from "@clauseo/components/lib/key-not-read.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { sx } from "@clauseo/components/lib/words-signin-expired.ts";
import { ui } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { AgentId } from "../../transport/protocol.ts";
import {
  CLAUDE,
  planLocked,
  signInWaits,
  type AgentSetup,
  type SetupState,
  type SetupStore,
} from "../../store/setup.ts";
import { installActs, planOf } from "./acts.ts";
import { gateView, installView, queuedView } from "./views.ts";

/** The line's presses: the install's, the room gate's Sign in, a found login's Use it, an unreadable key's Try again,
 * its Cancel, and the other plan's sign-in's Cancel. */
export type FootActs = InstallActs & {
  signIn: () => void;
  useFound: () => void;
  retryKey: () => void;
  cancel: () => void;
  cancelAhead: () => void;
};

/** A plan's line's presses: its install's, its "Sign in with {name}" (through the store's queue), Cancel, which stops
 * its download with its waiting sign-in while it isn't installed yet, and once installed stops a wait for the other
 * plan's sign-in or cancels its own sign-in; and "Cancel {name}'s sign-in", which cancels the other plan's sign-in
 * ahead of it. */
export const footActs = (store: SetupStore, agent: AgentId, state: SetupState): FootActs => ({
  ...installActs(store, agent, state.copied),
  signIn: () => store.signInTo(agent),
  // a found login's Use it, as the room gate's: use_found_login
  useFound: () => store.command({ use_found_login: { agent } }),
  // an unreadable key's Try again: the same start_sign_in, which desktop refuses and reads the key again, but no
  // choice of plan, so the papers line keeps naming both plans (the real-Mac run of #1162 lost "or ChatGPT")
  retryKey: () => store.signIn(agent, { again: true }),
  cancel: () => store.cancelSignIn(agent),
  cancelAhead: () => store.cancelAhead(agent),
});

/** The test ids of the signed-out line. */
const IDS = {
  line: "signin-foot",
  signIn: "signin-foot-sign-in",
  cancel: "signin-foot-cancel",
  cancelAhead: "signin-foot-cancel-ahead",
};

/** Whether the plan is on this computer: installed here, or by the firm. */
const installed = (setup: AgentSetup | undefined): boolean => {
  const install = setup?.install;
  if (install === undefined) return false;
  if (install === "managed") return true;
  return typeof install === "object" && "ready" in install && install.ready !== undefined;
};

/** The row as the first screen shows it beside the other plan's: it starts with its plan. A missing plan's size reads
 * "Claude · needs a download of about 110 MB", with no icon (only offered); words that name the plan already (getting
 * it ready, "{name} is ready", the queued line, most failures) keep their own words and no name in front; any other
 * words get the name in front as they are ("Claude · Finish signing in with your browser."). A ready plan's row leads
 * with the tick; the clock stays on rows that wait or download, and a cancelled sign-in has no icon. */
function named(
  line: WorkingState | null,
  lang: Lang,
  name: string,
  missing: number | null,
  lead?: "none" | "done",
): WorkingState | null {
  if (line === null || line.kind !== "wait") return line;
  const led = lead === undefined ? line : { ...line, lead };
  if (missing !== null)
    return {
      ...led,
      plan: name,
      words: fl(lang, "needs a download of about {mb} MB", { mb: missing }),
      lead: "none",
    };
  return line.words.includes(name) ? led : { ...led, plan: name };
}

/** Claude's note under "Finish signing in with your browser.": its sign-in always opens claude.ai's own sign-in page,
 * even when the lawyer is signed in there. Only for Claude, only while its row says that; null otherwise. */
export function browserNote(
  lang: Lang,
  agent: AgentId,
  line: WorkingState | null,
  setup: AgentSetup | undefined,
): string | null {
  if (agent !== CLAUDE || line === null) return null;
  const signIn = setup?.signIn;
  const waiting = typeof signIn === "object" && signIn.waiting !== undefined;
  return waiting &&
    line.kind === "wait" &&
    line.words === fl(lang, "Finish signing in with your browser.")
    ? fl(lang, "Claude may ask you to sign in again in your browser. Use your Claude account.")
    : null;
}

/** The install's line with a quiet Cancel after its own controls: the waiting sign-in's, which stops the download
 * too. */
function withCancel(line: WorkingState, lang: Lang, cancel: () => void): WorkingState {
  if (line.kind !== "wait") return line;
  return {
    ...line,
    actions: (
      <>
        {line.actions}
        <Button
          kind="quiet"
          size="small"
          className="working-act"
          onClick={cancel}
          data-testid={IDS.cancel}
        >
          {ui(lang, "Cancel")}
        </Button>
      </>
    ),
  };
}

/** The first screen's working line for `agent`, or null when there is nothing to say. */
export function welcomeFoot(
  lang: Lang,
  agent: AgentId,
  name: string,
  setup: AgentSetup | undefined,
  state: SetupState,
  online: boolean,
  acts: FootActs,
  ids: InstallTestIds,
): WorkingState | null {
  const locked = planLocked(state.locked, agent);
  const waits = signInWaits(state, agent) && !locked;
  const view0 = installView(setup, online);
  const install = installLine(lang, view0, name, { ...acts, named: true }, ids);
  // a waiting sign-in over its download: the download's own line and progress, and the Cancel that stops both
  if (waits && install !== null && view0?.kind !== "missing")
    return named(withCancel(install, lang, acts.cancel), lang, name, null);
  if (!waits && (install !== null || !installed(setup) || locked)) {
    return named(
      install,
      lang,
      name,
      view0?.kind === "missing" && view0.reinstall !== true ? view0.mb : null,
    );
  }
  // no clock time is said by the states shown here; a waiting sign-in says when its browser opens
  const view = waits
    ? queuedView(setup, state, agent, online)
    : gateView(setup, state, () => "", agent, online);
  if (view === null) return null;
  type Press = { words: string; press: () => void; testId: string; quiet?: boolean };
  const line = (words: string, kind: string, ...buttons: Press[]): WorkingState => ({
    kind: "wait",
    words,
    pill: null,
    actions: (
      <span className="fl-install" data-testid={IDS.line} data-state={kind}>
        {buttons.map((button) => (
          <Button
            key={button.testId}
            kind={button.quiet === true ? "quiet" : "tone"}
            size="small"
            className="working-act"
            onClick={button.press}
            data-testid={button.testId}
          >
            {button.words}
          </Button>
        ))}
      </span>
    ),
  });
  // a row with its note (said under it) and the words a group says, when it has them
  const noted = (row: WorkingState, note?: string, said?: string): WorkingState => ({
    ...row,
    ...(note === undefined ? {} : { note }),
    ...(said === undefined ? {} : { said }),
  });
  const signIn = {
    words: fl(lang, "Sign in with {name}", { name }),
    press: acts.signIn,
    testId: IDS.signIn,
  };
  const cancel = { words: ui(lang, "Cancel"), press: acts.cancel, testId: IDS.cancel, quiet: true };
  switch (view.kind) {
    // a cancelled sign-in says so, as Settings' card does, and offers it again (D-007: it read "{name} is ready" while
    // signed out); no tick, since the plan isn't ready to use, and no clock, since nothing waits: no icon, as an
    // offered row has none
    case "failed":
      if (view.code === "signin.cancelled")
        return named(
          line(errorWords(lang, "signin.cancelled"), "cancelled", {
            ...signIn,
            words: sx(lang, "Sign in again"),
          }),
          lang,
          name,
          null,
          "none",
        );
      // a saved key the keychain didn't give (agent.key_unreadable, or agent.key_denied after a Deny, each in its
      // own words): the gate's words and Try again (gate-failed.tsx; it picks no plan: acts.retryKey), never "is
      // ready" with a sign-in (VM row A-K6); desktop refuses the press
      // and reads the key again. The note says where the key can be changed; no tick, since the
      // plan can't be used until the key reads. Try again refused in another code says that code's
      // words after it, as the gate's refused line does (the same code is said once)
      if (isKeyNotRead(view.code)) {
        const key = errorWords(lang, view.code);
        const plans = sx(lang, "To change it, open Settings, then Plans.");
        const code = setup?.refused;
        const other = code === undefined || code === view.code ? null : errorWords(lang, code);
        return named(
          noted(
            line(key, "key-unreadable", {
              ...signIn,
              words: fl(lang, "Try again"),
              press: acts.retryKey,
            }),
            other === null ? plans : `${plans} ${other}`,
            other === null ? undefined : `${key} ${other}`,
          ),
          lang,
          name,
          null,
          "none",
        );
      }
      return named(
        line(fl(lang, "{name} is ready", { name }), view.kind, signIn),
        lang,
        name,
        null,
        "done",
      );
    // signed out, after the install: the plan is ready, and Sign in starts it
    case "signed-out":
      return named(
        line(fl(lang, "{name} is ready", { name }), view.kind, signIn),
        lang,
        name,
        null,
        "done",
      );
    case "queued": {
      // the other plan's sign-in ahead, with no line of its own here (a code, a switch): its Cancel, before this one's
      const ahead =
        view.after === undefined || view.ahead !== true
          ? []
          : [
              {
                words: fl(lang, "Cancel {name}'s sign-in", { name: view.after }),
                press: acts.cancelAhead,
                testId: IDS.cancelAhead,
                quiet: true,
              },
            ];
      return line(
        view.offline
          ? view.online === true
            ? fl(lang, "Your browser opens when setting up can continue.")
            : fl(lang, "Your browser opens when you reconnect.")
          : view.after !== undefined
            ? fl(lang, "Your browser opens for {name} after {other}'s sign-in.", {
                name,
                other: view.after,
              })
            : fl(lang, "Your browser opens when {name} is ready.", { name }),
        view.kind,
        ...ahead,
        cancel,
      );
    }
    case "waiting":
      return view.pasting
        ? null
        : named(
            line(fl(lang, "Finish signing in with your browser."), view.kind, cancel),
            lang,
            name,
            null,
          );
    // a login found on this computer (Claude Code signed in, Clauseo not yet): its line and Use it, as the room gate
    // says it, so the plan keeps its row on the first screen (before, it had none)
    case "login-found": {
      const app = APP[planOf(agent)];
      const found =
        view.email === undefined
          ? fl(lang, "Clauseo found your {name} account from {app} and will use it here.", {
              name,
              app,
            })
          : fl(
              lang,
              "Clauseo found your {name} account ({email}) from {app} and will use it here.",
              {
                name,
                app,
                email: view.email,
              },
            );
      const use = { words: fl(lang, "Use it"), press: acts.useFound, testId: "signin-foot-use" };
      return named(line(found, view.kind, use), lang, name, null, "none");
    }
    // the sign-in status check runs: its words, and Sign in busy until it answers
    case "checking":
      return checkingLine(lang, name, ids, true);
    default:
      return null;
  }
}
