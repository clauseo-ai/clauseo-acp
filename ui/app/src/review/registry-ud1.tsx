// UD1's copy review entries (the first run), apart from registry.tsx to keep that file under the size limit: each string
// in the real room over a FakeTransport that carries the setup wire, the first screen (no matter yet) or matter A open
// with the sign-in gate in its composer, in the state that shows it, pressed into where a press is needed. The strings
// from the approved design keep its Hindi and are approved "by": "design"; the new ones are English only (hi: later).
import { fl, type FirstLaunchKey } from "@clauseo/components/lib/words-first-launch.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  AgentId,
  InstallProgress,
  SettingsView,
  SetupEvent,
  SignIn,
} from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomClicking, RoomOver } from "./states.tsx";
import type { WordsRef } from "./words.ts";

const CLAUDE: AgentId = "claude-acp";
const CODEX: AgentId = "codex-acp";
/** A local time today: the host's times read as clock words. */
const at = (h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), h, m).getTime();
};
const AT = at(16, 42);
const HOSTS = ["registry.npmjs.org", "nodejs.org", "downloads.claude.ai"];
const LOG = ["Clauseo download log", "agent: claude-acp", "result: failed after 3 tries"];

const firstLaunchWords = (key: FirstLaunchKey): WordsRef => ({
  file: "lib/words-first-launch.ts",
  key,
  text: (lang) => fl(lang, key),
  later: fl.hi[key] === null,
});

const install = (agent: AgentId, state: InstallProgress): SetupEvent => ({
  agent_install_progress: { agent, state },
});
const signIn = (agent: AgentId, state: SignIn): SetupEvent => ({
  sign_in_changed: { agent, state },
});
const READY = install(CLAUDE, { ready: { version: "0.1.0" } });
const DOWNLOADING = install(CLAUDE, { downloading: { received: 61_000_000, total: 182_000_000 } });
const SIGNED_OUT = signIn(CLAUDE, { signed_out: {} });

type Scene = {
  /** No matter: the first screen. */
  first?: boolean;
  events: SetupEvent[];
  platform?: "mac" | "windows" | "linux";
  keys?: boolean;
  locked?: boolean;
  /** Controls to press, in order (CSS selectors). */
  clicks?: string[];
};

/** The room over a fake host with the setup wire, in the scene. */
function host(s: Scene): () => FakeTransport {
  return () =>
    new FakeTransport({
      platform: s.platform ?? "mac",
      openSettings: () => {},
      query: (query) => {
        if (query === "list_matters") return { matters: s.first === true ? [] : [MATTER] };
        if (query === "get_settings")
          return {
            settings: {
              ui: {
                theme: "system",
                font_size: 16,
                language: "en",
                lite: "automatic",
                motion: "system",
              },
              privacy: { counts: true, crash_reports: false, counts_started: false },
              locked: s.locked === true ? ["privacy.counts"] : [],
            } as unknown as SettingsView,
          };
        if (typeof query === "object" && "list_threads" in query) return { threads: [] };
        throw new Error(`not scripted: ${JSON.stringify(query)}`);
      },
      setup: { first: () => [{ setup_info: { keys_available: s.keys ?? true } }, ...s.events] },
    });
}

const render = (s: Scene) =>
  s.clicks === undefined
    ? () => <RoomOver host={host(s)} />
    : () => <RoomClicking host={host(s)} clicks={s.clicks ?? []} />;

const WELCOME: Scene = { first: true, events: [DOWNLOADING, SIGNED_OUT] };
const GATE: Scene = { events: [READY, SIGNED_OUT] };
const KEY: Scene = { events: [READY, SIGNED_OUT], clicks: [".fl-way-quiet"] };
const chatgpt = (state: SignIn, clicks: string[] = []): Scene => ({
  events: [
    READY,
    SIGNED_OUT,
    install(CODEX, { ready: { version: "0.1.0" } }),
    signIn(CODEX, state),
  ],
  clicks: [".fl-gate-ways .fl-way", ...clicks],
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
const found = (
  f: NonNullable<Extract<SignIn, { signed_out: unknown }>["signed_out"]>["found"],
) => ({
  events: [READY, signIn(CLAUDE, { signed_out: f === undefined ? {} : { found: f } })],
});
const blocked = install(CLAUDE, {
  failed: { code: "agent.install_blocked", hosts: HOSTS, log: LOG },
});
const failedInstall = install(CLAUDE, {
  failed: { code: "agent.install_failed", hosts: HOSTS, log: LOG },
});

/** Each string, the scene that shows it, in the order the brief lists the screens. Its place is its id (UD1-<n>), so a
 * retired string leaves a null where it stood and no id moves. */
const SCENES: ([FirstLaunchKey, Scene, string] | null)[] = [
  // the first screen (design)
  [
    "Clauseo works on your matters with the Claude or ChatGPT plan you already pay for.",
    WELCOME,
    "welcome",
  ],
  ["Open the first matter", WELCOME, "welcome"],
  ["Open the folder for a matter", WELCOME, "welcome"],
  [
    "Clauseo reads its papers on this computer first. Apart from anonymous counts, nothing leaves it until you ask.",
    WELCOME,
    "welcome",
  ],
  ["Start a new matter", WELCOME, "welcome"],
  ["Send anonymous counts", WELCOME, "welcome"],
  ["Send anonymous counts.", WELCOME, "welcome"],
  [
    "How many questions, which features, how long answers take. Never your papers, your questions or the answers.",
    WELCOME,
    "welcome",
  ],
  ["What's sent", WELCOME, "welcome"],
  // UD1-10, retired: a locked counts switch stands off with Settings' own words, no sentence of its own
  null,
  // the install, at the foot (design, then the new indeterminate wait)
  ["Getting {name} ready. About {mb} MB.", WELCOME, "welcome"],
  ["{name} is ready", { first: true, events: [install(CLAUDE, "installing"), READY] }, "welcome"],
  [
    "Setting up {name} on this computer.",
    { first: true, events: [install(CLAUDE, "installing")] },
    "welcome",
  ],
  ["Use the system proxy", { first: true, events: [blocked] }, "dl-blocked"],
  ["Ask your IT team", { first: true, events: [blocked] }, "dl-blocked"],
  [
    "Message copied",
    { first: true, events: [blocked], clicks: ['[data-testid="install-ask-it"]'] },
    "dl-blocked",
  ],
  [
    "Clauseo needs to download {name} to work. Please allow these addresses on the firm's network: {list}.",
    { first: true, events: [blocked] },
    "dl-blocked",
  ],
  [
    "{name} needs {need} MB free on this computer, and {free} MB is free.",
    {
      first: true,
      events: [
        install(CLAUDE, {
          failed: {
            code: "agent.install_disk_full",
            need_mb: 420,
            free_mb: 96,
            hosts: HOSTS,
            log: LOG,
          },
        }),
      ],
    },
    "dl-disk",
  ],
  ["{name} couldn't be downloaded.", { first: true, events: [failedInstall] }, "dl-error"],
  [
    "{name} couldn't be downloaded. The log is copied, with nothing from your matters in it.",
    {
      first: true,
      events: [failedInstall],
      clicks: ['[data-testid="install-foot"] button:last-child'],
    },
    "dl-error",
  ],
  ["Try again", { first: true, events: [failedInstall] }, "dl-error"],
  ["Details", { first: true, events: [failedInstall] }, "dl-error"],
  ["Your papers stay on this computer. {name} sees what each question sends it.", GATE, "plan"],
  // the plan, in the composer
  ["Sign in to {name} to ask about this matter", GATE, "plan"],
  ["Sign in", GATE, "plan"],
  ["Use {name} instead", GATE, "plan-chatgpt"],
  ["Use an API key or my firm's account", GATE, "plan-key"],
  [
    "Your browser opens when {name} is ready.",
    { events: [DOWNLOADING, signIn(CLAUDE, "queued")] },
    "plan-queued",
  ],
  [
    "Your browser opens when you reconnect.",
    {
      events: [
        install(CLAUDE, { waiting: { code: "agent.install_offline", hosts: HOSTS, log: LOG } }),
        signIn(CLAUDE, "queued"),
      ],
    },
    "plan-queued",
  ],
  ["API key", KEY, "plan-key"],
  ["Use this key", KEY, "plan-key"],
  [
    "A key from Claude or ChatGPT, or one from your firm's IT team. Billed to the key's own account.",
    KEY,
    "plan-key",
  ],
  ["Kept in this Mac's Keychain.", KEY, "plan-key"],
  ["Kept in Windows Credential Manager.", { ...KEY, platform: "windows" }, "plan-key"],
  ["Kept in this computer's keyring.", { ...KEY, platform: "linux" }, "plan-key"],
  [
    "This computer has no keyring to keep an API key in.",
    { ...KEY, platform: "linux", keys: false },
    "plan-key",
  ],
  ["Sign in with an Anthropic Console account", KEY, "plan-key"],
  // signing in, in the browser
  [
    "Finish signing in in your browser",
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
  // a login already on this computer
  [
    "Clauseo found your {name} account ({email}) from {app} and will use it here.",
    {
      events: [
        READY,
        signIn(CLAUDE, { login_found: { from: "claude_code", email: "test@example.com" } }),
      ],
    },
    "login-found",
  ],
  [
    "Clauseo found your {name} account from {app} and will use it here.",
    { events: [READY, signIn(CLAUDE, { login_found: { from: "claude_code" } })] },
    "login-found",
  ],
  [
    "Use it",
    { events: [READY, signIn(CLAUDE, { login_found: { from: "claude_code" } })] },
    "login-found",
  ],
  [
    "Sign in with a different account",
    { events: [READY, signIn(CLAUDE, { login_found: { from: "claude_code" } })] },
    "login-found",
  ],
  [
    "A different account signs in to Clauseo only. {app} stays as it is.",
    { events: [READY, signIn(CLAUDE, { login_found: { from: "claude_code" } })] },
    "login-found",
  ],
  [
    "Signed in through {app} as {email} · {plan}",
    signed({ email: "test@example.com", plan: "Claude Max", source: "reused" }),
    "signin-done",
  ],
  [
    "Signed in through {app} as {email}",
    signed({ email: "test@example.com", source: "reused" }),
    "signin-done",
  ],
  ["Signed in through {app}", signed({ source: "reused" }), "signin-done"],
  // a login seen that can't be shared, or reuse that stopped
  [
    "Claude Code on this computer signs in through your own Claude settings, which Clauseo keeps out of your matters.",
    found("claude_settings"),
    "plan-found",
  ],
  [
    "Codex on this computer keeps its sign-in where Clauseo can't share it.",
    chatgpt({ signed_out: { found: "codex_keyring" } }),
    "plan-found",
  ],
  [
    "Clauseo's safe workspace can't share Claude Code's sign-in on Windows yet.",
    { ...found("claude_windows_sandbox"), platform: "windows" },
    "plan-found",
  ],
  [
    "{app} on this computer is signed out, so Clauseo stopped using its sign-in.",
    found("reused_signed_out"),
    "plan-found",
  ],
  [
    "{app} on this computer now uses another account, so Clauseo stopped using its sign-in.",
    found("reused_changed"),
    "plan-found",
  ],
  [
    "Codex's sign-in on this computer changed, so Clauseo stopped using it.",
    chatgpt({ signed_out: { found: "codex_link_split" } }),
    "plan-found",
  ],
  [
    "{app}'s sign-in stopped working here, so Clauseo stopped using it.",
    found("reused_failed"),
    "plan-found",
  ],
  // ChatGPT's device code
  ["Use a code instead", chatgpt({ waiting: { since: AT, nudge: true } }), "signin-nudge"],
  [
    "Sign in to ChatGPT on that page and enter this code.",
    chatgpt({
      device_code: {
        url: "https://auth.openai.com/codex/device",
        code: "ABCD-1234",
        expires_at: at(16, 57),
      },
    }),
    "signin-device",
  ],
  [
    "The code works until {time}.",
    chatgpt({
      device_code: {
        url: "https://auth.openai.com/codex/device",
        code: "ABCD-1234",
        expires_at: at(16, 57),
      },
    }),
    "signin-device",
  ],
  [
    "Copy",
    chatgpt({
      device_code: {
        url: "https://auth.openai.com/codex/device",
        code: "ABCD-1234",
        expires_at: at(16, 57),
      },
    }),
    "signin-device",
  ],
  [
    "Open page",
    chatgpt({
      device_code: {
        url: "https://auth.openai.com/codex/device",
        code: "ABCD-1234",
        expires_at: at(16, 57),
      },
    }),
    "signin-device",
  ],
  [
    "To turn it on, open ChatGPT, then Settings, then Security, and turn on Enable device code sign-in for Codex, Excel, PowerPoint, and Word. Then choose Try again.",
    chatgpt({ failed: { code: "signin.device_code_off", at: AT } }),
    "signin-device-off",
  ],
  [
    "Sign in in the browser instead",
    chatgpt({ failed: { code: "signin.device_code_off", at: AT } }),
    "signin-device-off",
  ],
  // the other ways a sign-in stops
  ["Sign-in didn't finish.", { events: [READY, failed("signin.failed")] }, "signin-failed"],
  [
    "Clauseo can't read your firm's settings for {name}, so it won't sign in until they can be read.",
    { events: [READY, failed("signin.policy_unreadable")] },
    "signin-policy",
  ],
  ["Ask your IT team.", { events: [READY, failed("signin.policy_unreadable")] }, "signin-policy"],
];

export const UD1: readonly Entry[] = SCENES.flatMap((scene, i) =>
  scene === null
    ? []
    : [
        {
          id: `UD1-${String(i + 1).padStart(2, "0")}`,
          words: firstLaunchWords(scene[0]),
          screen: `first-launch/${scene[2]}`,
          render: render(scene[1]),
        },
      ],
);
