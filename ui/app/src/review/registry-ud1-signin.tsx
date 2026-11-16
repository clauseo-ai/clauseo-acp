// UD1's browser sign-in scenes and how each one ends (UD1-38 to UD1-54), apart from registry-ud1.tsx to keep that file
// under the size limit: the room over a FakeTransport that carries the setup wire, the matter's sign-in box open while
// signed out, with Claude's sign-in in the browser, its code pasted, or its result or failure shown.
import type { FirstLaunchKey } from "@clauseo/components/lib/words-first-launch.ts";
import type { AgentId, InstallProgress, SetupEvent, SignIn } from "../transport/protocol.ts";
import type { Scene } from "./registry-ud1-scene.tsx";

const CLAUDE: AgentId = "claude-acp";
const CODEX: AgentId = "codex-acp";
/** A local time today: the host's times read as clock words. */
const at = (h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), h, m).getTime();
};
const AT = at(16, 42);

const install = (agent: AgentId, state: InstallProgress): SetupEvent => ({
  agent_install_progress: { agent, state },
});
const signIn = (agent: AgentId, state: SignIn): SetupEvent => ({
  sign_in_changed: { agent, state },
});
const READY = install(CLAUDE, { ready: { version: "0.1.0" } });
const SIGNED_OUT = signIn(CLAUDE, { signed_out: {} });
const chatgpt = (state: SignIn, clicks: string[] = []): Scene => ({
  events: [
    READY,
    SIGNED_OUT,
    install(CODEX, { ready: { version: "0.1.0" } }),
    signIn(CODEX, state),
  ],
  clicks: ['.fl-gate-plan[data-plan="chatgpt"]', ...clicks],
});
const failed = (code: string): SetupEvent =>
  signIn(CLAUDE, { failed: { code: code as never, at: AT } });
const signed = (
  over: Partial<NonNullable<Extract<SignIn, { signed_in: unknown }>["signed_in"]>>,
): Scene => ({
  events: [
    READY,
    SIGNED_OUT,
    signIn(CLAUDE, { signed_in: { method: "plan", source: "own", at: AT, ...over } }),
  ],
});

/** The browser sign-in and how it ends, in the order the brief lists the screens. Each entry's place in this list is
 * its id in registry-ud1.tsx (UD1-<n>), so a retired string leaves a null where it stood. */
export const SIGNING_IN: ([FirstLaunchKey, Scene, string] | null)[] = [
  // signing in, in the browser
  [
    "Finish signing in with your browser.",
    { events: [READY, signIn(CLAUDE, { waiting: { since: AT, nudge: false } })] },
    "signin-wait",
  ],
  [
    "Didn't open?",
    { events: [READY, signIn(CLAUDE, { waiting: { since: AT, nudge: true } })] },
    "signin-nudge",
  ],
  [
    "Got a code instead?",
    { events: [READY, signIn(CLAUDE, { waiting: { since: AT, nudge: true } })] },
    "signin-nudge",
  ],
  [
    "Paste it",
    { events: [READY, signIn(CLAUDE, { waiting: { since: AT, nudge: true } })] },
    "signin-nudge",
  ],
  [
    "Code from your browser",
    {
      events: [READY, signIn(CLAUDE, { waiting: { since: AT, nudge: true } })],
      clicks: ['[data-testid="signin-paste"]'],
    },
    "signin-code",
  ],
  [
    "Clauseo passes the code to {name} and keeps no copy.",
    {
      events: [READY, signIn(CLAUDE, { waiting: { since: AT, nudge: true } })],
      clicks: ['[data-testid="signin-paste"]'],
    },
    "signin-code",
  ],
  [
    "Send",
    {
      events: [READY, signIn(CLAUDE, { waiting: { since: AT, nudge: true } })],
      clicks: ['[data-testid="signin-paste"]'],
    },
    "signin-code",
  ],
  [
    "Signed in as {email} · {plan}",
    signed({ email: "test@example.com", plan: "Claude Max" }),
    "signin-done",
  ],
  ["Signed in as {email}", signed({ email: "test@example.com" }), "signin-done"],
  ["Signed in to {name}", signed({}), "signin-done"],
  ["Using an API key, billed to its own account", signed({ method: "api_key" }), "signin-done"],
  ["Using your firm's account", signed({ method: "firm" }), "signin-done"],
  [
    "The sign-in link expired at {time}.",
    { events: [READY, failed("signin.link_expired")] },
    "signin-unfinished",
  ],
  ["See {name}'s plans", { events: [READY, failed("signin.no_plan")] }, "signin-noplan"],
  ["Use another account", { events: [READY, failed("signin.no_plan")] }, "signin-noplan"],
  [
    "Clauseo works with a Claude Pro or Max plan.",
    { events: [READY, failed("signin.no_plan")] },
    "signin-noplan",
  ],
  [
    "Clauseo works with a paid ChatGPT plan.",
    chatgpt({ failed: { code: "signin.no_plan", at: AT } }),
    "signin-noplan",
  ],
];
