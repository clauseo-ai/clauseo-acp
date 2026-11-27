// A plan installed whose sign-in desktop hasn't said yet: after an update or at startup, desktop's status check runs
// for some seconds (13 s on a Windows RC) before its first answer. Until then every surface says it is checking, with
// nothing to press, never "Sign in": a Sign in pressed meanwhile stopped that check, and the browser opened for a login
// that was already on this computer. Desktop's own `checking` reads the same.
//
// The wait for a first answer is bounded: CHECK_MS after the plan is installed with no sign-in state, the store marks
// it `checkLate`, and the plan reads signed out with its Sign in, so a desktop that never answers leaves no plan stuck.
// Desktop's explicit `checking` is not bounded: it is a check that runs, and it ends with an answer.
import type { AgentId } from "../transport/protocol.ts";
import type { AgentSetup } from "./setup.ts";

/** How long a plan installed with no sign-in state reads "checking" before it reads signed out. */
export const CHECK_MS = 30_000;

/** Installed here (the firm's runtime aside, whose sign-in desktop says with it): a status check runs for it. */
const installed = (setup: AgentSetup | undefined): boolean => {
  const install = setup?.install;
  return typeof install === "object" && "ready" in install && install.ready !== undefined;
};

/** The sign-in reads "checking": desktop's check running, or installed and no state heard yet, within CHECK_MS. */
export const signInChecking = (setup: AgentSetup | undefined): boolean =>
  setup?.signIn === "checking" ||
  (setup?.signIn === undefined && installed(setup) && setup?.checkLate !== true);

/** Installed, no sign-in state, and CHECK_MS gone by: it reads signed out, with its Sign in. */
export const signInLate = (setup: AgentSetup | undefined): boolean =>
  setup?.signIn === undefined && installed(setup) && setup?.checkLate === true;

/** The store's timers for the bound: `watch` after each change of a plan, `stop` when the stream stops. */
export function checkTimers(
  read: (id: AgentId) => AgentSetup | undefined,
  late: (id: AgentId) => void,
) {
  const timers = new Map<AgentId, ReturnType<typeof setTimeout>>();
  return {
    watch(id: AgentId) {
      const setup = read(id);
      // an answer heard: no bound to keep
      if (setup?.signIn !== undefined) {
        clearTimeout(timers.get(id));
        timers.delete(id);
        return;
      }
      if (timers.has(id) || setup?.checkLate === true || !signInChecking(setup)) return;
      timers.set(
        id,
        setTimeout(() => {
          if (read(id)?.signIn === undefined) late(id);
        }, CHECK_MS),
      );
    },
    stop() {
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
    },
  };
}
