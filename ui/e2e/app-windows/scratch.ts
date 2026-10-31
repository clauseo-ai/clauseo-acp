// The folders one test gives the app, so it never touches the real data or Documents folders: a
// temporary root holding the data folder (`CLAUSEO_DATA_DIR`, with a `settings.json` whose
// `matters.default_location` is the scratch `matters/` folder, where the host makes its Scratch
// matter, and, with a scenario, whose `agents.claude` starts `fake_agent` on it), the matter folder
// (`E2E_MATTER_DIR`, which the e2e build's pick_folder returns instead of opening the dialog), and
// one WebView2 data folder per launch.
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

/** The settings key the window's threads use: `agents.claude`. */
export const AGENT_ID = "claude";

/** What a test asks of its folders. */
export interface ScratchOptions {
  /** A scenario bundled with fake_agent (`fake_agent --scenario <name>`), or null for no agent. */
  scenario: string | null;
  /** Files the matter folder starts with: `/`-separated relative path to content. */
  matterFiles: Readonly<Record<string, string>>;
  /** More arguments for fake_agent after `--scenario <name>`, e.g. `--pace-ms 5`. */
  agentArgs?: readonly string[];
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
}

/**
 * The `agents` section that starts `command --scenario <scenario> [...extra]` as `agents.claude`.
 */
export function agentSettings(
  command: string,
  scenario: string,
  extra: readonly string[] = [],
): Record<string, unknown> {
  if (!isAbsolute(command)) {
    throw new Error(`the agent command must be an absolute path (S5), not ${command}`);
  }
  return { agents: { [AGENT_ID]: { command, args: ["--scenario", scenario, ...extra] } } };
}

/**
 * The whole `settings.json`: new matters in `mattersDir` (S5 `matters.default_location`, absolute;
 * without it the host's Scratch matter lands in the real Documents/Clauseo), plus the agent when
 * there is one.
 */
export function scratchSettings(
  mattersDir: string,
  agent: { command: string; scenario: string; args?: readonly string[] } | null,
): Record<string, unknown> {
  if (!isAbsolute(mattersDir)) {
    throw new Error(`matters.default_location must be an absolute path, not ${mattersDir}`);
  }
  return {
    matters: { default_location: mattersDir },
    ...(agent === null ? {} : agentSettings(agent.command, agent.scenario, agent.args)),
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
    const settings = scratchSettings(
      mattersDir,
      agentCommand === null || options.scenario === null
        ? null
        : { command: agentCommand, scenario: options.scenario, args: options.agentArgs ?? [] },
    );
    writeFileSync(join(dataDir, "settings.json"), `${JSON.stringify(settings, null, 2)}\n`);
    return { root, dataDir, matterDir, mattersDir, agentCommand };
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
 * The variables every launch gets. `E2E_CONFIRM_ALWAYS=accept` makes the e2e build answer the
 * native "always allow" confirmation itself, which CDP cannot click.
 */
export function scratchEnv(scratch: Scratch): Record<string, string> {
  return {
    CLAUSEO_DATA_DIR: scratch.dataDir,
    E2E_MATTER_DIR: scratch.matterDir,
    E2E_CONFIRM_ALWAYS: "accept",
  };
}

/** Removes the root. WebView2 and the agent may hold files for a moment after they end. */
export function removeScratch(scratch: Scratch): void {
  rmSync(scratch.root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
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
