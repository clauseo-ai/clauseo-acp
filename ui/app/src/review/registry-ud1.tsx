// UD1's copy review entries (the first run), apart from registry.tsx to keep that file under the size limit: each string
// in the real room over a FakeTransport that carries the setup wire, the first screen (no matter yet) or matter A open
// with the sign-in gate in its composer, in the state that shows it, pressed into where a press is needed. The strings
// from the approved design keep its Hindi and are approved "by": "design"; the new ones are English only (hi: later).
// the Hindi words in at once, as review/words.ts has them: each ref reads its Hindi when made
import "@clauseo/components/lib/words-hi.ts";
import { fl, type FirstLaunchKey } from "@clauseo/components/lib/words-first-launch.ts";
import type { AgentId, InstallProgress, SetupEvent, SignIn } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { render, type Scene } from "./registry-ud1-scene.tsx";
import { SIGNING_IN } from "./registry-ud1-signin.tsx";
import * as T from "./registry-ud1-tools.ts";
import { errorWords, threadWords, type WordsRef } from "./words.ts";

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

/** The first screen with Claude downloading, beside ChatGPT's row with its size. */
const WELCOME: Scene = {
  first: true,
  events: [DOWNLOADING, SIGNED_OUT, install(CODEX, { missing: { download_bytes: 139_000_000 } })],
};
const GATE: Scene = { events: [READY, SIGNED_OUT] };
const KEY: Scene = { events: [READY, SIGNED_OUT], clicks: [".fl-way-quiet"] };
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
const found = (
  f: NonNullable<Extract<SignIn, { signed_out: unknown }>["signed_out"]>["found"],
) => ({
  events: [READY, signIn(CLAUDE, { signed_out: f === undefined ? {} : { found: f } })],
});
const SWITCHING: Scene = {
  events: [READY, signIn(CLAUDE, { switching: { since: AT, stuck: false } })],
};
const CHECKING: Scene = { events: [READY, signIn(CLAUDE, "checking")] };
/** Claude's install went missing after Clauseo installed it (#859), on the first screen. */
const REINSTALL: Scene = {
  first: true,
  events: [install(CLAUDE, { missing: { download_bytes: 182_400_000, installed_before: true } })],
};
const STUCK: Scene = { events: [READY, signIn(CLAUDE, { switching: { since: AT, stuck: true } })] };
/** The key panel open while signed out, the switch begun, then a key sent and refused with command.conflict. */
const SWITCH_CONFLICT: Scene = {
  events: [READY, SIGNED_OUT],
  refuse: "command.conflict",
  steps: [
    { press: ".fl-way-quiet" },
    { play: signIn(CLAUDE, { switching: { since: AT, stuck: false } }) },
    { type: "sk-ant-api03-example" },
    { press: ".fl-gate .room-field-ask" },
  ],
};
/** Sign in pressed while a key is in use: desktop refuses it with signin.key_in_use. */
const KEY_IN_USE: Scene = {
  events: [READY, SIGNED_OUT],
  refuse: "signin.key_in_use",
  steps: [{ press: ".fl-gate .room-field-ask" }],
};
const blocked = install(CLAUDE, {
  failed: { code: "agent.install_blocked", hosts: HOSTS, log: LOG },
});
const failedInstall = install(CLAUDE, {
  failed: { code: "agent.install_failed", hosts: HOSTS, log: LOG },
});
const offline = install(CLAUDE, {
  waiting: { code: "agent.install_offline", hosts: HOSTS, log: LOG },
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
  [
    "Needs a download of about {mb} MB",
    { first: true, events: [install(CLAUDE, { missing: { download_bytes: 182_400_000 } })] },
    "welcome",
  ],
  // installed but signed out: the words on the foot's line, with the plan's Sign in (once signed in, the same words
  // stand alone, for an install that finished in this window)
  [
    "{name} is ready",
    { first: true, events: [install(CLAUDE, "installing"), READY, SIGNED_OUT] },
    "welcome",
  ],
  ["Setting up {name}…", { first: true, events: [install(CLAUDE, "installing")] }, "welcome"],
  ["Use the system proxy", { first: true, events: [blocked] }, "dl-blocked"],
  // UD1-15, retired: the blocked download's button reads Copy addresses (UC-144); "Copy details for IT" stays IT's
  null,
  [
    "Message copied",
    { first: true, events: [blocked], clicks: ['[data-testid="install-ask-it"]'] },
    "dl-blocked",
  ],
  [
    "Clauseo needs to download {name}. These addresses must be reachable from this computer: {list}.",
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
  [
    "Your papers stay on this computer. {name} sees the parts it reads to answer you.",
    GATE,
    "plan",
  ],
  // the plan, in the composer
  // UD1-24, retired: the matter's sign-in box names both plans now (UD1-95)
  null,
  ["Sign in", { events: [READY, failed("signin.cancelled")] }, "signin-cancelled"],
  // signed out, the box offers both plans' own Sign in; "Use … instead" stays where a sign-in stopped
  ["Use {name} instead", { events: [READY, failed("signin.cancelled")] }, "plan-chatgpt"],
  // UD1-27, retired: the quiet way to the key is "Use an API key instead" (UC-141); the firm's address is its policy's
  null,
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
  // UD1-30, retired: the key's field names its maker (UC-139, UC-140)
  null,
  ["Use this key", KEY, "plan-key"],
  // UD1-32, retired: the key's help names its maker (UC-145; UC-146, ChatGPT's, is shown in Plans)
  null,
  ["Kept in this Mac's Keychain.", KEY, "plan-key"],
  ["Kept in Windows Credential Manager.", { ...KEY, platform: "windows" }, "plan-key"],
  ["Kept in this computer's keyring.", { ...KEY, platform: "linux" }, "plan-key"],
  [
    "This computer has no keyring to keep an API key in.",
    { ...KEY, platform: "linux", keys: false },
    "plan-key",
  ],
  ["Sign in with an Anthropic Console account", KEY, "plan-key"],
  // UD1-38 to UD1-54: the browser sign-in and how it ends, in registry-ud1-signin.tsx, in its place here
  ...SIGNING_IN,
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
    "Clauseo's Safe workspace can't share Claude Code's sign-in on Windows yet.",
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
    "Sign in with ChatGPT on that page and enter this code.",
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
  [
    "Your firm manages these settings.",
    { events: [READY, failed("signin.policy_unreadable")] },
    "signin-policy",
  ],
  // switching accounts (0048, #493), and a refusal meanwhile
  ["Switching accounts.", SWITCHING, "signin-switching"],
  [
    "Clauseo is still stopping the old login. It keeps trying by itself, so there's nothing you need to do.",
    SWITCHING,
    "signin-switching",
  ],
  ["If this goes on, quit Clauseo and open it again.", STUCK, "signin-switching-stuck"],
  [
    "Clauseo is still switching accounts. Try again in a moment.",
    SWITCH_CONFLICT,
    "signin-switching-conflict",
  ],
  // UD1-84: the refusal line for any other code (REFUSAL, below), whose words are words-errors.ts's
  null,
  // UD1-85 to UD1-87: the Offline and Failed views' Try again and Use the system proxy, added after the null above so
  // no id moves
  ["Try again", { first: true, events: [offline] }, "dl-offline"],
  ["Use the system proxy", { first: true, events: [offline] }, "dl-offline"],
  ["Use the system proxy", { first: true, events: [failedInstall] }, "dl-error"],
  // UD1-88: the download's line, its bytes in whole MB. The live region hears only the state,
  // Getting {name} ready…, never these bytes
  ["Getting {name} ready… {done} MB of {total} MB", WELCOME, "welcome"],
  // UD1-89: the sign-in status check still running, at start and through its silent retries
  ["Checking whether you're signed in to {name}…", CHECKING, "signin-checking"],
  // UD1-90 and UD1-91: the chosen plan's install went missing after Clauseo installed it (#859), and its Install
  ["{plan} isn't installed on this computer any more.", REINSTALL, "reinstall"],
  ["Install {plan}", REINSTALL, "reinstall"],
  // UD1-92 to UD1-95: both plans side by side, on the first screen and in the matter's sign-in box
  ["Sign in with the plan you pay for: Claude, ChatGPT or both.", T.BOTH, "welcome-both"],
  [
    "Your papers stay on this computer. Claude or ChatGPT sees the parts it reads to answer you.",
    T.BOTH,
    "welcome-both",
  ],
  ["Sign in with {name}", T.BOTH, "welcome-both"],
  ["Sign in with Claude or ChatGPT to ask about this matter", GATE, "plan"],
  // UD1-96: a plan's row on the first screen, after its name and " · "
  ["needs a download of about {mb} MB", T.BOTH, "welcome-both"],
  // UD1-97: Claude's browser sign-in note, under its first-screen row (also in the matter's gate and Plans' card)
  [
    "Claude may ask you to sign in again in your browser. Use your Claude account.",
    { first: true, events: [READY, signIn(CLAUDE, { waiting: { since: AT, nudge: false } })] },
    "welcome-browser-note",
  ],
  // UD1-98 to UD1-101: the document tools
  ["Also downloads Clauseo's document tools ({size}).", T.TOOLS_MISSING, "tools-missing"],
  ["Downloading document tools… {done} of {size}", T.TOOLS_DOWNLOADING, "tools-downloading"],
  ["Clauseo couldn't download its document tools.", T.TOOLS_FAILED, "tools-failed"],
  ["Windows blocked Clauseo's document tools on this computer.", T.TOOLS_BLOCKED, "tools-blocked"],
  // UD1-102: the document tools offline, in the plans' own offline words (TOOLS_OFFLINE, below)
  null,
];

/** The gate's refusal line for any other code: its code's own sentence, here a sign-in refused while a key is in use.
 * Its id is the null SCENES keeps for it. */
const REFUSAL: Entry = {
  id: "UD1-84",
  words: errorWords(
    "Clauseo is using your API key. To sign in with your plan instead, open Settings, then Plans, and remove the key.",
  ),
  screen: "first-launch/signin-refused",
  render: render(KEY_IN_USE),
};

/** The document tools offline: the plans' own offline sentence, in the null SCENES keeps for it. */
const TOOLS_OFFLINE: Entry = {
  id: "UD1-102",
  words: errorWords("You're offline. Setting up waits for the connection."),
  screen: "first-launch/tools-waiting",
  render: render(T.TOOLS_WAITING),
};

/** The key's words named by its maker, and the quiet way to it (UC-139, UC-141 and UC-145; UC-140 and UC-146,
 * ChatGPT's, are shown in Plans, registry-ud2-plans.tsx), and a blocked download's Copy addresses (UC-144). */
const UC_KEY: Entry[] = (
  [
    { id: "UC-139", key: "Anthropic API key", scene: KEY, screen: "plan-key" },
    { id: "UC-141", key: "Use an API key instead", scene: GATE, screen: "plan-key" },
    {
      id: "UC-144",
      key: "Copy addresses",
      scene: { first: true, events: [blocked] },
      screen: "dl-blocked",
    },
    {
      id: "UC-145",
      key: "A key from the Anthropic Console. Billed to that account.",
      scene: KEY,
      screen: "plan-key",
    },
  ] satisfies { id: string; key: FirstLaunchKey; scene: Scene; screen: string }[]
).map(({ id, key, scene, screen }) => ({
  id,
  words: firstLaunchWords(key),
  screen: `first-launch/${screen}`,
  render: render(scene),
}));

/** UC-153: ChatGPT ready while Claude's sign-in is under way, on the first screen: sign-ins go one at a time, so its
 * row says its browser opens after Claude's (the parallel installs; UC's id, this file's scene). */
const AFTER_SIGN_IN: Entry = {
  id: "UC-153",
  words: firstLaunchWords("Your browser opens for {name} after {other}'s sign-in."),
  screen: "first-launch/plan-after",
  render: render({
    first: true,
    events: [
      READY,
      signIn(CLAUDE, { waiting: { since: AT, nudge: false } }),
      install(CODEX, { ready: { version: "0.1.0" } }),
      signIn(CODEX, "queued"),
    ],
  }),
};

/** UC-230: ChatGPT's waiting sign-in cancelled while it downloads, beside Claude's download: desktop drops only the
 * sign-in, so its row says the download carries on (the parallel installs; UC's id, this file's scene). */
const STILL_DOWNLOADING: Entry = {
  id: "UC-230",
  words: firstLaunchWords(
    "Sign-in cancelled. {name} is still downloading; sign in when it's ready.",
  ),
  screen: "first-launch/plan-cancel-downloading",
  render: render({
    first: true,
    events: [
      install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
      signIn(CLAUDE, "queued"),
      install(CODEX, { downloading: { received: 90_000_000, total: 139_000_000 } }),
      signIn(CODEX, "queued"),
    ],
    clicks: ['[data-testid="plan-row"][data-agent="codex-acp"] [data-testid="signin-foot-cancel"]'],
  }),
};

/** The composer's line while the plan runs on the lawyer's API key, named by its maker (UC-147, UC-148). */
const UC_KEY_LINE: Entry[] = [
  {
    id: "UC-147",
    words: threadWords("Uses your Anthropic API key"),
    screen: "first-launch/signin-done",
    render: render(signed({ method: "api_key" })),
  },
  {
    id: "UC-148",
    words: threadWords("Uses your OpenAI API key"),
    screen: "first-launch/signin-done",
    render: render({
      events: [
        install(CLAUDE, { missing: { download_bytes: 182_000_000 } }),
        SIGNED_OUT,
        install(CODEX, { ready: { version: "0.1.0" } }),
        signIn(CODEX, { signed_in: { method: "api_key", source: "own", at: AT } }),
      ],
    }),
  },
];

export const UD1: readonly Entry[] = [
  ...SCENES.flatMap((scene, i) =>
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
  ),
  REFUSAL,
  TOOLS_OFFLINE,
  ...UC_KEY,
  AFTER_SIGN_IN,
  STILL_DOWNLOADING,
  ...UC_KEY_LINE,
];
