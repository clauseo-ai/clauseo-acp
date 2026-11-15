// The probe's first run with both plans' installs (probe-main.tsx's ?installs=), fake data only, for the parallel
// installs' screenshots: both downloading at different percentages (?installs=both), ChatGPT ready and its browser
// sign-in open while Claude still downloads (?installs=chatgpt-signing), and ChatGPT ready while Claude's sign-in is
// under way, so its line says its browser opens after Claude's, (?installs=after); Claude's sign-in waiting at desktop behind ChatGPT's browser sign-in, for a matter's gate with
// ChatGPT's line and its Cancel (?installs=gate-browser), or behind ChatGPT's device code, which has no line, so Claude's
// line offers "Cancel ChatGPT's sign-in" (?installs=gate-code); and
// ChatGPT back to not installed after its download's Cancel, beside Claude's download (?installs=cancelled). With
// ?matters=none the first screen shows them; with a matter, the composer's gate and both lines. Not shipped.
import type { InstallProgress, SetupEvent, SignIn } from "../../transport/protocol.ts";

const CLAUDE = "claude-acp";
const CODEX = "codex-acp";
const MB = 1_000_000;

const install = (agent: string, state: InstallProgress): SetupEvent => ({
  agent_install_progress: { agent, state },
});
const signIn = (agent: string, state: SignIn): SetupEvent => ({
  sign_in_changed: { agent, state },
});
const downloading = (received: number, total: number): InstallProgress => ({
  downloading: { received: received * MB, total: total * MB },
});
const READY: InstallProgress = { ready: { version: "0.1.0" } };
const BROWSER: SignIn = { waiting: { since: Date.now(), nudge: false } };

const PAGES: Record<string, SetupEvent[]> = {
  both: [
    { setup_info: { keys_available: true } },
    install(CLAUDE, downloading(13, 165)),
    signIn(CLAUDE, "queued"),
    install(CODEX, downloading(92, 139)),
    signIn(CODEX, "queued"),
  ],
  "chatgpt-signing": [
    { setup_info: { keys_available: true } },
    install(CLAUDE, downloading(58, 165)),
    signIn(CLAUDE, "queued"),
    install(CODEX, READY),
    signIn(CODEX, BROWSER),
  ],
  after: [
    { setup_info: { keys_available: true } },
    install(CLAUDE, READY),
    signIn(CLAUDE, BROWSER),
    install(CODEX, READY),
    signIn(CODEX, "queued"),
  ],
  "gate-browser": [
    { setup_info: { keys_available: true } },
    install(CLAUDE, READY),
    signIn(CLAUDE, "queued"),
    install(CODEX, READY),
    signIn(CODEX, BROWSER),
  ],
  "gate-code": [
    { setup_info: { keys_available: true } },
    install(CLAUDE, READY),
    signIn(CLAUDE, "queued"),
    install(CODEX, READY),
    signIn(CODEX, {
      device_code: {
        url: "https://auth.openai.com/codex/device",
        code: "ABCD-1234",
        expires_at: Date.now() + 900_000,
      },
    }),
  ],
  cancelled: [
    { setup_info: { keys_available: true } },
    install(CLAUDE, downloading(13, 165)),
    signIn(CLAUDE, "queued"),
    install(CODEX, { missing: { download_bytes: 139 * MB } }),
    signIn(CODEX, { signed_out: {} }),
  ],
};

/** The setup host for ?installs=, or null on any other page. */
export function installsProbe(
  name: string | null,
): { first: () => SetupEvent[]; command: () => void } | null {
  const events = name === null ? undefined : PAGES[name];
  return events === undefined ? null : { first: () => events, command: () => {} };
}
