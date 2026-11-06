// The folders one test gives the app, so it never touches the real data or Documents folders: a
// temporary root holding the data folder (`CLAUSEO_DATA_DIR`, with a `settings.json` whose
// `matters.default_location` is the scratch `matters/` folder, where the host makes its Scratch
// matter, and, with a scenario, whose `agents.claude-acp` starts `fake_agent` on it), the matter folder
// (`E2E_MATTER_DIR`, which the e2e build's pick_folder returns instead of opening the dialog), and
// the WebView2 data folder of each launch: new per launch, or one per worker reused (`Profile`).
// Unless a test asks for the first run, desktop's agents start set up (`SETUP_AGENTS`): each one's
// `plans.<id>` names a provider, which desktop shows as signed in by the firm from its first setup
// event, and `E2E_AGENTS_MANAGED` has the e2e build take their installs as the firm's, so nothing
// downloads and the first run's sign-in gate never takes the composer's Send (#494).
// Pure file work, no app: scratch.test.ts runs it on any OS.
//
//   E2E_FAKE_AGENT    absolute path to fake_agent(.exe); default: beside E2E_APP

import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, relative, sep } from "node:path";

/** The settings key the window's threads use: `agents.claude-acp`. */
export const AGENT_ID = "claude-acp";

/**
 * The agents desktop sets up (its snapshot's), which every test but a first run's starts with
 * installed and signed in: so a conversation on either plan, the test's agent or not, sends.
 */
export const SETUP_AGENTS: readonly string[] = [AGENT_ID, "codex-acp"];

/** What a test asks of its folders. */
export interface ScratchOptions {
  /** A scenario bundled with fake_agent (`fake_agent --scenario <name>`), or null for no agent. */
  scenario: string | null;
  /** Files the matter folder starts with: `/`-separated relative path to content. */
  matterFiles: Readonly<Record<string, string>>;
  /** More arguments for fake_agent after `--scenario <name>`, e.g. `--pace-ms 5`. */
  agentArgs?: readonly string[];
  /** More agents beside `agents.claude-acp`: agent id (`codex-acp`) to the scenario it plays. */
  otherAgents?: Readonly<Record<string, string>>;
  /**
   * True leaves desktop's agents as on a lawyer's first run: not installed and signed out, so
   * the first run's install line and sign-in gate show. False (the default) starts them set up.
   */
  firstRun?: boolean;
  /**
   * Agents whose sign-in the e2e build scripts (`E2E_FAKE_SIGNIN`): `ok` plays Queued, Waiting,
   * then signed in; `fail` ends failed. Each starts installed (`E2E_AGENTS_MANAGED`) but signed
   * out: it is left out of `plans`, so its sign-in card shows.
   */
  fakeSignIn?: Readonly<Record<string, FakeSignIn>>;
}

/** How a scripted sign-in ends. */
export type FakeSignIn = "ok" | "fail";

/**
 * `E2E_FAKE_SIGNIN_HOLD_MS` for a scripted sign-in: how long `Waiting` lasts. The app's default
 * is 1.5 s, too short for a spec to read Waiting before the sign-in ends; 5 s cannot race.
 */
export const FAKE_SIGNIN_HOLD_MS = 5000;

/** `E2E_FAKE_SIGNIN`'s value for `scripts`: `<agent>=<script>`, comma-separated. */
export function fakeSignInVar(scripts: Readonly<Record<string, FakeSignIn>>): string {
  return Object.entries(scripts)
    .map(([agent, script]) => `${agent}=${script}`)
    .join(",");
}

/** One test's folders. */
export interface Scratch {
  /** The temporary root; everything below is removed after the test. */
  root: string;
  /** `CLAUSEO_DATA_DIR`. */
  dataDir: string;
  /** `E2E_MATTER_DIR`: the folder the window opens as a matter. */
  matterDir: string;
  /** `matters.default_location`: new matters (the host's Scratch matter) go here, not Documents. */
  mattersDir: string;
  /** The fake_agent the settings start, or null when the test has no scenario. */
  agentCommand: string | null;
  /** The agents that start set up (`E2E_AGENTS_MANAGED`, `plans`): none on a first run. */
  setUp: readonly string[];
  /** `E2E_FAKE_SIGNIN`'s value, empty when no sign-in is scripted. */
  fakeSignIn?: string;
}

/**
 * The `agents` section that starts `command --scenario <scenario> [...extra]` as
 * `agents.claude-acp`, and `command --scenario <its scenario>` as each of `others`.
 */
export function agentSettings(
  command: string,
  scenario: string,
  extra: readonly string[] = [],
  others: Readonly<Record<string, string>> = {},
): Record<string, unknown> {
  if (!isAbsolute(command)) {
    throw new Error(`the agent command must be an absolute path (S5), not ${command}`);
  }
  if (AGENT_ID in others) {
    throw new Error(`${AGENT_ID} plays the test's scenario; it cannot be one of the other agents`);
  }
  const agents: Record<string, unknown> = {
    [AGENT_ID]: { command, args: ["--scenario", scenario, ...extra] },
  };
  for (const [id, other] of Object.entries(others)) {
    agents[id] = { command, args: ["--scenario", other] };
  }
  return { agents };
}

/**
 * The `plans` section that signs `agents` in: `provider: anthropic`, which desktop counts as the
 * firm's plan (`SignedIn` by `firm`) and which adds no variable to the agent's launch, so
 * fake_agent runs as without it.
 */
export function signedInPlans(agents: readonly string[]): Record<string, unknown> {
  return agents.length === 0
    ? {}
    : { plans: Object.fromEntries(agents.map((id) => [id, { provider: "anthropic" }])) };
}

/**
 * The whole `settings.json`: new matters in `mattersDir` (S5 `matters.default_location`, absolute;
 * without it the host's Scratch matter lands in the real Documents/Clauseo), plus the agent when
 * there is one, plus the plans that sign `setUp` in.
 */
export function scratchSettings(
  mattersDir: string,
  agent: {
    command: string;
    scenario: string;
    args?: readonly string[];
    others?: Readonly<Record<string, string>>;
  } | null,
  setUp: readonly string[] = [],
): Record<string, unknown> {
  if (!isAbsolute(mattersDir)) {
    throw new Error(`matters.default_location must be an absolute path, not ${mattersDir}`);
  }
  return {
    matters: { default_location: mattersDir },
    ...(agent === null
      ? {}
      : agentSettings(agent.command, agent.scenario, agent.args, agent.others)),
    ...signedInPlans(setUp),
  };
}

/**
 * fake_agent's path: `E2E_FAKE_AGENT`, else `fake_agent(.exe)` beside the app, where
 * `cargo build --release --target <t> -p clauseo-fake_agent` puts it next to the e2e build.
 */
export function fakeAgentPath(env: NodeJS.ProcessEnv, appPath: string, windows: boolean): string {
  const set = env["E2E_FAKE_AGENT"];
  if (set !== undefined && set !== "") {
    return set;
  }
  return join(dirname(appPath), windows ? "fake_agent.exe" : "fake_agent");
}

/** The joined path of a `/`-separated relative path under `folder`; refuses absolute and `..`. */
function under(folder: string, path: string): string {
  const parts = path.split("/");
  if (path === "" || isAbsolute(path) || parts.some((part) => part === ".." || part === "")) {
    throw new Error(`a matter file needs a plain relative path, not ${JSON.stringify(path)}`);
  }
  return join(folder, ...parts);
}

/** Makes the folders under a new temporary root in `base`. `agent` is asked only with a scenario. */
export function makeScratch(
  options: ScratchOptions,
  agent: () => string,
  base = tmpdir(),
): Scratch {
  // The long path: a Windows temp path may hold 8.3 short names, and the host compares folders
  // after canonicalising them.
  const root = realpathSync.native(mkdtempSync(join(base, "clauseo-e2e-")));
  try {
    // Siblings: a matter may not contain or sit inside the data folder or the matters root.
    const dataDir = join(root, "data");
    const matterDir = join(root, "matter");
    const mattersDir = join(root, "matters");
    mkdirSync(dataDir);
    mkdirSync(matterDir);
    mkdirSync(mattersDir);
    for (const [path, content] of Object.entries(options.matterFiles)) {
      const file = under(matterDir, path);
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(file, content);
    }
    let agentCommand: string | null = null;
    if (options.scenario !== null) {
      agentCommand = agent();
      if (!existsSync(agentCommand)) {
        throw new Error(
          `fake_agent is not at ${agentCommand}; build it (cargo build -p clauseo-fake_agent) or set E2E_FAKE_AGENT`,
        );
      }
    }
    const setUp = options.firstRun === true ? [] : SETUP_AGENTS;
    const settings = scratchSettings(
      mattersDir,
      agentCommand === null || options.scenario === null
        ? null
        : {
            command: agentCommand,
            scenario: options.scenario,
            args: options.agentArgs ?? [],
            others: options.otherAgents ?? {},
          },
      setUp.filter((id) => options.fakeSignIn?.[id] === undefined),
    );
    writeFileSync(join(dataDir, "settings.json"), `${JSON.stringify(settings, null, 2)}\n`);
    const fakeSignIn = fakeSignInVar(options.fakeSignIn ?? {});
    return { root, dataDir, matterDir, mattersDir, agentCommand, setUp, fakeSignIn };
  } catch (error) {
    rmSync(root, { recursive: true, force: true });
    throw error;
  }
}

/** A new, empty WebView2 data folder for launch number `n`, so its port file is never stale. */
export function webviewFolder(scratch: Scratch, n: number): string {
  const folder = join(scratch.root, `webview-${n}`);
  mkdirSync(folder);
  return folder;
}

/**
 * Which WebView2 profile (`WEBVIEW2_USER_DATA_FOLDER`) a launch gets:
 * - `fresh`: a new, empty folder for every launch, as on a user's first launch (the default; the
 *   smoke tests and every spec but the scoreboard's);
 * - `warm`: one folder per worker, made by its first launch and reused by every later one, as on a
 *   later launch of an installed app (the scoreboard, whose warm-up play makes it: #173).
 */
export type Profile = "fresh" | "warm";

/** The WebView2 profile reused by every `warm` launch of one worker. */
export interface WarmProfile {
  /** The folder; made on the first call, the same on every later one. */
  folder(): string;
  /** The launches started on it so far. */
  launches: number;
}

/** A {@link WarmProfile} that can be removed at the worker's end. */
export interface RemovableWarmProfile extends WarmProfile {
  /**
   * Removes the folder, if it was made, once no browser holds it ({@link waitForFreeProfile}, up to
   * `timeoutMs`). Never throws, since the results are written by then: a profile still locked when
   * the wait ends, or a folder that cannot be removed, goes to `warn` and stays on disk.
   */
  remove(warn: (message: string) => void, timeoutMs?: number): Promise<void>;
  /**
   * Removes the folder as {@link remove} does, then starts over: the next `folder()` makes a new
   * one and `launches` is 0 again, so the next launch is once more launch 1 on a new profile. For a
   * warm-up launch that failed to start ({@link retryWarmUp}).
   */
  reset(warn: (message: string) => void, timeoutMs?: number): Promise<void>;
}

/** A {@link WarmProfile} under a new temporary folder in `base`, made only when first asked for. */
export function warmProfile(base = tmpdir()): RemovableWarmProfile {
  let folder: string | null = null;
  return {
    launches: 0,
    folder() {
      folder ??= realpathSync.native(mkdtempSync(join(base, "clauseo-e2e-webview-")));
      return folder;
    },
    async reset(warn, timeoutMs = PROFILE_FREE_TIMEOUT_MS) {
      await this.remove(warn, timeoutMs);
      folder = null;
      this.launches = 0;
    },
    async remove(warn, timeoutMs = PROFILE_FREE_TIMEOUT_MS) {
      if (folder === null) return;
      if (!(await waitForFreeProfile(folder, timeoutMs))) {
        warn(
          `a browser process still holds the warm WebView2 profile ${folder} ${timeoutMs} ms after ` +
            `the last launch ended; removing it anyway`,
        );
      }
      try {
        rmSync(folder, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
      } catch (error) {
        warn(`the warm WebView2 profile ${folder} could not be removed: ${String(error)}`);
      }
    },
  };
}

/** The WebView2 data folder of one launch, and which launch on that folder it is (1 for the first). */
export interface LaunchProfile {
  profile: Profile;
  folder: string;
  /** 1 when the folder is new (always, for `fresh`); above 1 when an earlier launch left it. */
  launch: number;
}

/**
 * The WebView2 data folder for launch `n` on `scratch`: a new `webview-<n>` folder for `fresh`, the
 * worker's one folder for `warm`. Counts the launch on `warm`.
 */
export function launchProfile(
  profile: Profile,
  scratch: Scratch,
  n: number,
  warm: WarmProfile,
): LaunchProfile {
  if (profile === "fresh") return { profile, folder: webviewFolder(scratch, n), launch: 1 };
  warm.launches += 1;
  return { profile, folder: warm.folder(), launch: warm.launches };
}

/** An error's message on one line, for a `::warning` workflow command. */
export function oneLine(error: unknown): string {
  return (error instanceof Error ? error.message : String(error)).replace(/\r?\n/g, " ");
}

/**
 * Tries `attempt`, and once more when the first try fails with an error `retryable` accepts (the
 * warm-up launch failing to start: fixture.ts). Before the second try, `recover` cleans up what the
 * first left, then `report` gets the first error's message on one line, so the run still shows a
 * launch that passed only on its retry. Any other error, or a second failure, throws; a second
 * failure names the first in its message and has the second as `cause`.
 */
export async function retryWarmUp<T>(
  attempt: () => Promise<T>,
  retryable: (error: unknown) => boolean,
  recover: (error: unknown) => Promise<void>,
  report: (message: string) => void,
): Promise<T> {
  let first: unknown;
  try {
    return await attempt();
  } catch (error) {
    if (!retryable(error)) throw error;
    first = error;
  }
  const firstMessage = oneLine(first);
  try {
    await recover(first);
  } catch (error) {
    throw new Error(`cleaning up after the failed warm-up launch failed (${firstMessage})`, {
      cause: error,
    });
  }
  report(firstMessage);
  try {
    return await attempt();
  } catch (error) {
    throw new Error(
      `the warm-up launch failed again on its one retry: ${oneLine(error)} (first try: ${firstMessage})`,
      { cause: error },
    );
  }
}

/** The file WebView2 writes its DevTools port to, in its data folder. */
export function portFile(userDataFolder: string): string {
  return join(userDataFolder, "EBWebView", "DevToolsActivePort");
}

/**
 * The lock Chromium's browser process holds on its profile while it runs: opened without delete
 * sharing and with delete-on-close (Chromium's process_singleton_win.cc), so it exists exactly as
 * long as a browser runs on the folder, and goes even when the process is killed.
 */
export function profileLock(userDataFolder: string): string {
  return join(userDataFolder, "EBWebView", "lockfile");
}

/** How long a reused profile may stay locked by the launch before it. */
export const PROFILE_FREE_TIMEOUT_MS = 15_000;
const PROFILE_FREE_RETRY_MS = 100;

/**
 * Waits until no browser process holds `userDataFolder` (its {@link profileLock} gone): true once
 * free, false when the lock is still there after `timeoutMs`.
 */
export async function waitForFreeProfile(
  userDataFolder: string,
  timeoutMs = PROFILE_FREE_TIMEOUT_MS,
): Promise<boolean> {
  const lock = profileLock(userDataFolder);
  const deadline = performance.now() + timeoutMs;
  while (existsSync(lock)) {
    if (performance.now() >= deadline) return false;
    await new Promise((resolve) => setTimeout(resolve, PROFILE_FREE_RETRY_MS));
  }
  return true;
}

/**
 * Readies a reused WebView2 data folder for the next launch: waits until no browser process holds
 * it (its {@link profileLock} gone), since a new launch would otherwise join the old browser
 * process instead of starting its own, then deletes the last launch's port file, so the port the
 * fixture reads is always the new launch's. Throws when the lock stays past `timeoutMs`.
 */
export async function readyReusedProfile(
  userDataFolder: string,
  timeoutMs = PROFILE_FREE_TIMEOUT_MS,
): Promise<void> {
  if (!(await waitForFreeProfile(userDataFolder, timeoutMs))) {
    throw new Error(
      `a browser process still holds the reused WebView2 profile ${userDataFolder} ` +
        `${timeoutMs} ms after its last launch ended (${profileLock(userDataFolder)} is there)`,
    );
  }
  // force: no port file is fine (the launch before never wrote one); a file still held throws.
  rmSync(portFile(userDataFolder), { force: true });
}

/**
 * The variables every launch gets. `E2E_CONFIRM_ALWAYS=accept` makes the e2e build answer the
 * native "always allow" confirmation itself, which CDP cannot click; `E2E_AGENTS_MANAGED` has it
 * take the set-up agents' installs as the firm's, so none downloads (empty on a first run).
 */
export function scratchEnv(scratch: Scratch): Record<string, string> {
  return {
    CLAUSEO_DATA_DIR: scratch.dataDir,
    E2E_MATTER_DIR: scratch.matterDir,
    E2E_CONFIRM_ALWAYS: "accept",
    E2E_AGENTS_MANAGED: scratch.setUp.join(","),
    ...(scratch.fakeSignIn
      ? {
          E2E_FAKE_SIGNIN: scratch.fakeSignIn,
          E2E_FAKE_SIGNIN_HOLD_MS: String(FAKE_SIGNIN_HOLD_MS),
        }
      : {}),
  };
}

/**
 * The processes that may still hold a scratch folder after the app ended: on Windows, the
 * WebView2 processes whose command line names it (holders.ts).
 */
export interface Holders {
  /** The PIDs of the processes that hold `root`. */
  find(root: string): Promise<number[]>;
  /** Ends `pids`; one already gone is no error. */
  kill(pids: readonly number[]): Promise<void>;
}

/** How long a scratch folder's {@link Holders} may take to exit on their own before the kill. */
export const HOLDERS_EXIT_TIMEOUT_MS = 30_000;
/** How long killed holders may take to go. */
export const HOLDERS_KILL_TIMEOUT_MS = 10_000;
const HOLDERS_POLL_MS = 1_000;

/** How {@link removeScratch} waits for a held root; the defaults are the fixture's. */
export interface RemoveOptions {
  /** Who may hold the root; without it, a held root throws at once, as any other error does. */
  holders?: Holders;
  /** Hears of every wait for holders, every kill and every holder left after it. */
  warn?: (message: string) => void;
  exitTimeoutMs?: number;
  killTimeoutMs?: number;
  pollMs?: number;
  /** Removes the root: `rmSync` with about 2 s of retries. The tests give a stand-in. */
  remove?: (root: string) => void;
}

/** `rmSync` of `root`, retrying for about 2 s while a file is held. */
function removeRoot(root: string): void {
  rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}

/** Whether `error` is a file still held open: what a process that has not let go makes. */
function isHeld(error: unknown): boolean {
  const code = error instanceof Error ? (error as NodeJS.ErrnoException).code : undefined;
  return code === "EPERM" || code === "EBUSY" || code === "ENOTEMPTY";
}

/**
 * Removes the root. WebView2 and the agent may hold files for a moment after they end, so the
 * removal retries for about 2 s. Under load WebView2's processes can outlive the app by minutes
 * (clauseo-acp-private measurements/pr340-e2e-loop: up to 4 per folder, over 2 min after the
 * app had gone), so when the removal still fails on a held file and `holders` is given, it waits
 * up to `exitTimeoutMs` for them to exit, kills those still running, waits up to `killTimeoutMs`
 * for them to go, and removes again. Throws the last removal's error.
 */
export async function removeScratch(scratch: Scratch, options: RemoveOptions = {}): Promise<void> {
  const {
    holders,
    warn = () => {},
    exitTimeoutMs = HOLDERS_EXIT_TIMEOUT_MS,
    killTimeoutMs = HOLDERS_KILL_TIMEOUT_MS,
    pollMs = HOLDERS_POLL_MS,
    remove = removeRoot,
  } = options;
  const root = scratch.root;
  try {
    remove(root);
    return;
  } catch (error) {
    if (holders === undefined || !isHeld(error)) throw error;
  }
  const start = performance.now();
  const left = await waitForHolders(holders, root, exitTimeoutMs, pollMs, warn);
  if (left === null) {
    // Not known who holds it (warned): the removal's own error, if any, says the rest.
  } else if (left.length === 0) {
    warn(
      `${root} was still held after the app ended; no WebView2 process named it ` +
        `${Math.round(performance.now() - start)} ms later`,
    );
  } else {
    warn(
      `WebView2 processes ${left.join(", ")} still held ${root} ${exitTimeoutMs} ms after the ` +
        `app ended; killing them`,
    );
    await holders.kill(left).catch((error: unknown) => {
      warn(`killing ${left.join(", ")} failed: ${oneLine(error)}`);
    });
    const after = await waitForHolders(holders, root, killTimeoutMs, pollMs, warn);
    if (after !== null && after.length > 0) {
      warn(`processes ${after.join(", ")} still held ${root} after the kill; removing it anyway`);
    }
  }
  remove(root);
}

/**
 * Asks `holders` for `root`'s holders every `pollMs` until there are none or `timeoutMs` passes:
 * the PIDs still there, none once they are gone. A failed question goes to `warn` and gives null,
 * so the removal is still tried and its own error stands.
 */
async function waitForHolders(
  holders: Holders,
  root: string,
  timeoutMs: number,
  pollMs: number,
  warn: (message: string) => void,
): Promise<number[] | null> {
  const deadline = performance.now() + timeoutMs;
  for (;;) {
    let pids: number[];
    try {
      pids = await holders.find(root);
    } catch (error) {
      warn(`listing the processes that hold ${root} failed: ${oneLine(error)}`);
      return null;
    }
    if (pids.length === 0 || performance.now() >= deadline) return pids;
    await new Promise((resolve) => setTimeout(resolve, pollMs));
  }
}

/**
 * The files under `folder` (`/`-separated relative path to content), leaving out `.clauseo`,
 * the matter's own folder. Empty folders do not show, so an undo that leaves one still matches.
 */
export function readTree(folder: string): Record<string, string> {
  const files: Record<string, string> = {};
  const walk = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (dir === folder && entry.name === ".clauseo") {
          continue;
        }
        walk(path);
      } else if (entry.isFile()) {
        files[relative(folder, path).split(sep).join("/")] = readFileSync(path, "utf8");
      }
    }
  };
  walk(folder);
  return files;
}
