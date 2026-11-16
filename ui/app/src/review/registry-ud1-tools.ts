// UD1's first-screen scenes for both plans side by side (UD1-92 to UD1-94) and the document tools (UD1-98 to UD1-102),
// apart from registry-ud1.tsx to keep it under the size limit: SetupInfo's `python` in each state the line shows.
import type {
  AgentId,
  InstallProgress,
  PythonProgress,
  SetupEvent,
  SignIn,
} from "../transport/protocol.ts";

const CLAUDE: AgentId = "claude-acp";
const CODEX: AgentId = "codex-acp";
const install = (agent: AgentId, state: InstallProgress): SetupEvent => ({
  agent_install_progress: { agent, state },
});
const signIn = (agent: AgentId, state: SignIn): SetupEvent => ({
  sign_in_changed: { agent, state },
});

/** A fresh computer: both plans not here yet, side by side with their sizes. */
export const BOTH = {
  first: true,
  events: [
    install(CLAUDE, { missing: { download_bytes: 112_000_000 } }),
    install(CODEX, { missing: { download_bytes: 139_000_000 } }),
  ],
};
/** SetupInfo again, now with the document tools' state. */
const tools = (python: PythonProgress): SetupEvent => ({
  setup_info: { keys_available: true, python },
});
/** A fresh computer, both plans with their sizes, and the document tools' size said once under them. */
export const TOOLS_MISSING = {
  first: true,
  events: [
    install(CLAUDE, { missing: { download_bytes: 112_000_000 } }),
    install(CODEX, { missing: { download_bytes: 139_000_000 } }),
    tools({ missing: { download_bytes: 120_000_000 } }),
  ],
};
/** Claude ready (and usable): the document tools download after it, under ChatGPT's row still offering its own. */
const toolsAfterClaude = (python: PythonProgress) => ({
  first: true,
  events: [
    install(CLAUDE, { ready: { version: "0.1.0" } }),
    signIn(CLAUDE, { signed_out: {} }),
    install(CODEX, { missing: { download_bytes: 139_000_000 } }),
    tools(python),
  ],
});
export const TOOLS_DOWNLOADING = toolsAfterClaude({
  downloading: { received: 42_000_000, total: 120_000_000 },
});
export const TOOLS_FAILED = toolsAfterClaude({
  failed: { cause: "failed", code: "agent.install_failed", log: ["result: failed after 3 tries"] },
});
export const TOOLS_BLOCKED = toolsAfterClaude({
  failed: { cause: "run_blocked", blocked_by: "app_control", log: ["start refused"] },
});
export const TOOLS_WAITING = toolsAfterClaude({
  waiting: { code: "agent.install_offline", hosts: [], log: [] },
});
