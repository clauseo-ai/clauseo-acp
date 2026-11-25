// A plan the lawyer has, its install redone after a newer Clauseo installed over an older one (UC-1501 to UC-1508;
// store/agent-update.ts): Plans in the settings window over FakeTransport, ChatGPT's install events flagged
// installed_before (stage D): not started, downloading, setting up, paused, cancelled or failed. In place of "Add ChatGPT" and
// "Needs a download of about {mb} MB." The first screen and the composer say the same words through the same line.
import type { FirstLaunchKey } from "@clauseo/components/lib/words-first-launch.ts";
import type { AgentId, InstallProgress, SetupEvent } from "../../transport/protocol.ts";
import type { Entry } from "../entry.ts";
import { SettingsOver, type CellSetup } from "../states-settings.tsx";
import { firstLaunchWords } from "../words.ts";

const CLAUDE: AgentId = "claude-acp";
const CODEX: AgentId = "codex-acp";
const AT = new Date(2026, 10, 24, 16, 32).getTime();

const install = (agent: AgentId, state: InstallProgress): SetupEvent => ({
  agent_install_progress: { agent, state },
});
/** Stage D's event for an added plan (its flag not in today's bindings: agent-update.ts). */
const updating = (agent: AgentId, state: InstallProgress): SetupEvent =>
  ({ agent_install_progress: { agent, state, installed_before: true } }) as SetupEvent;
const HAD: InstallProgress = { missing: { download_bytes: 201_000_000, installed_before: true } };
const HAD0: InstallProgress = { missing: { download_bytes: 0, installed_before: true } };
const DOWN: InstallProgress = { downloading: { received: 84_000_000, total: 201_000_000 } };

/** Claude signed in, and ChatGPT's update through `states`, with no sign-in state (stage D pushes none). */
const scene = (...states: InstallProgress[]): CellSetup => ({
  first: [
    install(CLAUDE, { ready: { version: "0.2.0" } }),
    {
      sign_in_changed: {
        agent: CLAUDE,
        state: {
          signed_in: {
            email: "r.mehra@mehrachambers.in",
            plan: "Claude Max",
            method: "plan",
            source: "own",
            at: AT,
          },
        },
      },
    },
    ...states.map((s) => updating(CODEX, s)),
  ],
});

const entry = (id: string, key: FirstLaunchKey, setup: CellSetup): Entry => ({
  id,
  words: firstLaunchWords(key),
  screen: "settings/plans",
  width: 960,
  height: 1000,
  render: (lang) => <SettingsOver section="plans" lang={lang} setup={setup} />,
});

const UC_AGENT_UPDATE: Entry[] = [
  entry("UC-1501", "Updating {name}… {done} MB of {total} MB", scene(DOWN)),
  entry("UC-1502", "Updating {name}…", scene(DOWN, "installing")),
  entry("UC-1503", "{name}'s update is paused.", scene(DOWN, "paused")),
  entry("UC-1504", "{name}'s update stopped.", scene(DOWN, HAD)),
  entry("UC-1506", "{name} needs its new version, about {mb} MB.", scene(HAD)),
  entry("UC-1507", "{name} needs its new version.", scene(HAD0)),
  entry("UC-1508", "Download", scene(HAD)),
  entry(
    "UC-1505",
    "{name}'s update couldn't be downloaded.",
    scene(DOWN, { failed: { code: "agent.install_failed", hosts: [], log: ["x"] } }),
  ),
];
export default UC_AGENT_UPDATE;
