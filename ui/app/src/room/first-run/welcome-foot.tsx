// One plan's working line on the first screen, at its foot where the composer will stand (one for each plan, Claude's
// then ChatGPT's): the install's line while the plan isn't here yet (install.tsx, with "Sign in to {name}" while it is
// missing), and once it is installed but not signed in, the plan's "Sign in to {name}" in the same place (busy while the sign-in status check runs), so a lawyer can get the plan going
// without opening a matter. No gate stands on the first screen: the line says the gate's own words for a sign-in under
// way and offers its Cancel, never a second Sign in; the states only a matter's gate walks (a device code, a login
// found, switching accounts) leave the foot as it was. A plan waiting for the other plan's download or sign-in to end
// (the setup store's `queued`) says the queued sign-in's words, with a Cancel that stops its wait. A plan the firm's
// policy locks shows no control. Settings' Plans has its own Sign in again.
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import {
  checkingLine,
  installLine,
  type InstallActs,
  type InstallTestIds,
} from "@clauseo/components/clauseo/first-launch/install.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { sx } from "@clauseo/components/lib/words-signin-expired.ts";
import { ui } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { AgentId } from "../../transport/protocol.ts";
import { CLAUDE, planLocked, type AgentSetup, type SetupState } from "../../store/setup.ts";
import { gateView, installView } from "./views.ts";

/** The line's presses: the install's, the room gate's Sign in, and its Cancel. */
export type FootActs = InstallActs & { signIn: () => void; cancel: () => void };

/** The test ids of the signed-out line. */
const IDS = { line: "signin-foot", signIn: "signin-foot-sign-in", cancel: "signin-foot-cancel" };

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
 * with the tick; the clock stays on rows that wait or download. */
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

/** The first screen's working line for `agent`, or null when there is nothing to say. */
export function welcomeFoot(
  lang: Lang,
  agent: AgentId,
  name: string,
  setup: AgentSetup | undefined,
  state: SetupState,
  acts: FootActs,
  ids: InstallTestIds,
): WorkingState | null {
  const locked = planLocked(state.locked, agent);
  const waiting = state.queued === agent && !locked;
  const view0 = installView(setup);
  const install = waiting ? null : installLine(lang, view0, name, { ...acts, named: true }, ids);
  if (install !== null || (!waiting && (!installed(setup) || locked)))
    return named(
      install,
      lang,
      name,
      view0?.kind === "missing" && view0.reinstall !== true ? view0.mb : null,
    );
  // no clock time is said by the states shown here; the plan waiting on the other is the queued sign-in's
  const view = waiting
    ? ({ kind: "queued", offline: false } as const)
    : gateView(setup, state, () => "");
  if (view === null) return null;
  const line = (
    words: string,
    kind: string,
    button: { words: string; press: () => void; testId: string; quiet?: boolean },
  ): WorkingState => ({
    kind: "wait",
    words,
    pill: null,
    actions: (
      <span className="fl-install" data-testid={IDS.line} data-state={kind}>
        <Button
          kind={button.quiet === true ? "quiet" : "tone"}
          size="small"
          className="working-act"
          onClick={button.press}
          data-testid={button.testId}
        >
          {button.words}
        </Button>
      </span>
    ),
  });
  const signIn = {
    words: fl(lang, "Sign in to {name}", { name }),
    press: acts.signIn,
    testId: IDS.signIn,
  };
  const cancel = { words: ui(lang, "Cancel"), press: acts.cancel, testId: IDS.cancel, quiet: true };
  switch (view.kind) {
    // a cancelled sign-in says so, as Settings' card does, and offers it again (D-007: it read "{name} is ready" while
    // signed out); no tick, since the plan isn't ready to use
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
        );
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
    case "queued":
      return line(
        view.offline
          ? fl(lang, "Your browser opens when you reconnect.")
          : fl(lang, "Your browser opens when {name} is ready.", { name }),
        view.kind,
        cancel,
      );
    case "waiting":
      return view.pasting
        ? null
        : named(
            line(fl(lang, "Finish signing in with your browser."), view.kind, cancel),
            lang,
            name,
            null,
          );
    // the sign-in status check runs: its words, and Sign in busy until it answers
    case "checking":
      return checkingLine(lang, name, ids, true);
    default:
      return null;
  }
}
