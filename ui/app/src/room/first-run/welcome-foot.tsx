// The first screen's working line, at its foot where the composer will stand: the install's line while the plan isn't
// here yet (install.tsx, with the room gate's Sign in while it is missing), and once it is installed but not signed in,
// the plan's Sign in in the same place (busy while the sign-in status check runs), so a lawyer can get the plan going
// without opening a matter. No gate stands on the first screen: the line says the gate's own words for a sign-in under
// way and offers its Cancel, never a second Sign in; the states only a matter's gate walks (a device code, a login
// found, switching accounts) leave the foot as it was. A plan the firm's policy locks shows no control. Settings'
// Plans has its own Sign in again.
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import {
  checkingLine,
  installLine,
  type InstallActs,
  type InstallTestIds,
} from "@clauseo/components/clauseo/first-launch/install.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { ui } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { AgentId } from "../../transport/protocol.ts";
import { planLocked, type AgentSetup, type SetupState } from "../../store/setup.ts";
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
  const install = installLine(lang, installView(setup), name, acts, ids);
  if (install !== null || !installed(setup) || planLocked(state.locked, agent)) return install;
  // no clock time is said by the states shown here
  const view = gateView(setup, state, () => "");
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
  const signIn = { words: fl(lang, "Sign in"), press: acts.signIn, testId: IDS.signIn };
  const cancel = { words: ui(lang, "Cancel"), press: acts.cancel, testId: IDS.cancel, quiet: true };
  switch (view.kind) {
    // signed out, or a sign-in that stopped: the plan is ready, and Sign in starts it again
    case "signed-out":
    case "failed":
      return line(fl(lang, "{name} is ready", { name }), view.kind, signIn);
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
        : line(fl(lang, "Finish signing in with your browser."), view.kind, cancel);
    // the sign-in status check runs: its words, and Sign in busy until it answers
    case "checking":
      return checkingLine(lang, name, ids);
    default:
      return null;
  }
}
