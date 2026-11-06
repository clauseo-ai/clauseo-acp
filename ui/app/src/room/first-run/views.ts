// The setup store's state as the first run's parts show it: the install as the working line says it, and the sign-in
// as the gate says it. Pure, so each state's screen is tested from the host's words alone.
import type { GateRefusal, GateView } from "@clauseo/components/clauseo/first-launch/gate.tsx";
import type { InstallView } from "@clauseo/components/clauseo/first-launch/install.tsx";
import { isSignedIn, type AgentSetup, type SetupState } from "../../store/setup.ts";

/** The install as the working line shows it, from the host's InstallProgress; null with nothing to say. */
export function installView(setup: AgentSetup | undefined): InstallView | null {
  const install = setup?.install;
  if (install === undefined || install === "managed") return null;
  if (install === "installing") return { kind: "installing" };
  // paused: another plan's download goes first; it says where it stands, still
  if (install === "paused")
    return setup?.mb === undefined ? null : { kind: "missing", mb: setup.mb };
  if ("missing" in install && install.missing !== undefined)
    return { kind: "missing", mb: setup?.mb ?? 0 };
  if ("downloading" in install && install.downloading !== undefined)
    return { kind: "downloading", mb: setup?.mb ?? 0, ...install.downloading };
  if ("waiting" in install && install.waiting !== undefined) return { kind: "offline" };
  if ("failed" in install && install.failed !== undefined) {
    const f = install.failed;
    if (f.code === "agent.install_blocked") return { kind: "blocked", hosts: f.hosts };
    if (f.code === "agent.install_disk_full")
      return { kind: "disk", need: f.need_mb ?? 0, free: f.free_mb ?? 0 };
    return { kind: "failed", log: f.log };
  }
  return null;
}

/** The gate for a plan's sign-in, or null when it is signed in or nothing is known of it (a host before R20). */
export function gateView(
  setup: AgentSetup | undefined,
  state: SetupState,
  clock: (at: number) => string,
): GateView | null {
  const signIn = setup?.signIn;
  if (signIn === undefined || isSignedIn(signIn)) return null;
  if (state.keyOpen) return { kind: "key" };
  if (signIn === "queued") {
    const install = setup?.install;
    const offline =
      typeof install === "object" && "waiting" in install && install.waiting !== undefined;
    return { kind: "queued", offline };
  }
  if ("signed_out" in signIn && signIn.signed_out !== undefined)
    return { kind: "signed-out", found: signIn.signed_out.found };
  if ("waiting" in signIn && signIn.waiting !== undefined)
    return { kind: "waiting", nudge: signIn.waiting.nudge, pasting: state.pasting };
  if ("device_code" in signIn && signIn.device_code !== undefined)
    return {
      kind: "device",
      code: signIn.device_code.code,
      until: clock(signIn.device_code.expires_at),
    };
  if ("login_found" in signIn && signIn.login_found !== undefined)
    return { kind: "login-found", email: signIn.login_found.email };
  if ("switching" in signIn && signIn.switching !== undefined)
    return { kind: "switching", stuck: signIn.switching.stuck };
  if ("failed" in signIn && signIn.failed !== undefined) {
    // the agent's own refusal for no sign-in is the plan screen again
    if (signIn.failed.code === "agent.auth_required") return { kind: "signed-out" };
    return { kind: "failed", code: signIn.failed.code, at: clock(signIn.failed.at) };
  }
  return { kind: "signed-out" };
}

/** The last setup command desktop refused for the plan, as the gate's line says it; null with none. A
 * `command.conflict` while the plan is switching accounts says only that the switch isn't done. */
export function refusalView(setup: AgentSetup | undefined): GateRefusal | null {
  if (setup?.refused === undefined) return null;
  const signIn = setup.signIn;
  const switching = typeof signIn === "object" && signIn.switching !== undefined;
  return { code: setup.refused, switching };
}
