// A plan the lawyer already has, its install redone after an update of Clauseo: "Updating {plan}", never "Add {plan}",
// and no sign-in prompt until it is ready. The store keeps it per plan (AgentSetup's `update`) from the install
// events, through to Ready.
//
// How the host says it (stage D):
// - SetupEvent's AgentInstallProgress carries `installed_before: true` whenever the settings hold the plan as added
//   (left out when false), and InstallProgress's Missing keeps its own `installed_before` (#859). Either one means
//   "already has it".
// - An update emits no Missing: Downloading { received: 0, total }, its ticks, Installing, then Ready, each with the
//   flag; offline Waiting, Failed (blocked, a full disk, a failure, setting up) can stop it.
// - Missing with installed_before: an update that hasn't started (today's desktop never starts it by itself, and stage
//   D's automatic install may fail to start), so `due`: "{plan} needs its new version", with Download. After this
//   window saw the download run (`ran`), the same Missing is a download that stopped (cancelled): Try again.
// - After Ready the backend's own status check says checking, then signed in; it starts no sign-in, nor does the window.
import type { InstallProgress, SetupEvent } from "../transport/protocol.ts";

type Progress = NonNullable<
  Extract<SetupEvent, { agent_install_progress: unknown }>["agent_install_progress"]
>;

/** Whether the install event says the plan was added here before: the event's flag or Missing's. */
export function installedBefore(event: Progress): boolean {
  // TODO(stage D update PR): the event's `installed_before` is not in today's bindings; once it is, read it as
  // `event.installed_before === true` and drop this cast. The one place the window reads it.
  const flagged = (event as Progress & { installed_before?: boolean }).installed_before === true;
  const install = event.state;
  const missing =
    typeof install === "object" &&
    "missing" in install &&
    install.missing !== undefined &&
    install.missing.installed_before === true;
  return flagged || missing;
}

/** Where a plan's update stands: `due` (not started as far as this window saw), `ran` (its install has run). */
export type AgentUpdate = "due" | "ran";

/** The plan's update after this install event: said so by the event, or still under way since an event that said so
 * (a host before stage D says it on Missing alone); never once ready or the firm's. */
export function updateOf(was: AgentUpdate | undefined, event: Progress): AgentUpdate | undefined {
  const install: InstallProgress = event.state;
  if (install === "managed") return undefined;
  if (typeof install === "object" && "ready" in install && install.ready !== undefined)
    return undefined;
  const missing =
    typeof install === "object" && "missing" in install && install.missing !== undefined;
  // a Missing that doesn't say it: a plan not added (its card's "Add {plan}")
  if (missing) return installedBefore(event) ? (was ?? "due") : undefined;
  return installedBefore(event) || was !== undefined ? "ran" : undefined;
}
