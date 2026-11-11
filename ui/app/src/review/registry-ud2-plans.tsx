// UD2's copy review entries for Plans (settings.html#plans): the settings window (settings-app.tsx) over FakeTransport
// with the setup wire, open at Plans, at the window's size, one state per row of the brief's Plans table. A confirm
// shows after the press that opens it; "Signed out of Claude." after the sign-out is confirmed and the host answers.
// The room's scan, first read and expired card are part A's (registry-ud2.tsx). English only (hi: later).
import type { SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import type { AgentId, InstallProgress, SetupEvent, SignIn } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { PressInOrder } from "./press-words.tsx";
import { SettingsOver, type CellSetup } from "./states-settings.tsx";
import { firstLaunchWords, settingsWords, type WordsRef } from "./words.ts";

/** The settings window, 960 wide (DESIGN.md), tall enough for both plans' blocks. */
const WINDOW = { width: 960, height: 1000 } as const;

const CLAUDE: AgentId = "claude-acp";
const CODEX: AgentId = "codex-acp";
const AT = new Date(2026, 10, 6, 16, 32).getTime();
const READY: InstallProgress = { ready: { version: "0.1.0" } };

const install = (agent: AgentId, state: InstallProgress): SetupEvent => ({
  agent_install_progress: { agent, state },
});
const signIn = (agent: AgentId, state: SignIn): SetupEvent => ({
  sign_in_changed: { agent, state },
});
const signed = (
  over: Partial<NonNullable<Extract<SignIn, { signed_in: unknown }>["signed_in"]>> = {},
): SignIn => ({
  signed_in: {
    email: "r.mehra@mehrachambers.in",
    plan: "Claude Max",
    method: "plan",
    source: "own",
    at: AT,
    ...over,
  },
});
/** Claude in `claude`, ChatGPT added and in `codex`. */
const both = (claude: SignIn, codex: SignIn = { signed_out: {} }): SetupEvent[] => [
  install(CLAUDE, READY),
  signIn(CLAUDE, claude),
  install(CODEX, READY),
  signIn(CODEX, codex),
];

type Scene = {
  setup: CellSetup;
  /** Controls pressed in order, each once it is on the page (CSS selectors; a dialog's are in its portal). */
  press?: string[];
};

const OWN: Scene = {
  setup: { first: both(signed(), signed({ plan: "ChatGPT Plus" })) },
};
const SIGN_OUT_ASK: Scene = { ...OWN, press: ['[data-testid="plans-sign-out"]'] };
const SIGNED_OUT_HERE: Scene = {
  setup: {
    first: both(signed()),
    answer: (c) => ("sign_out" in c ? [signIn(CLAUDE, { signed_out: {} })] : []),
  },
  press: ['[data-testid="plans-sign-out"]', '[data-testid="plans-sign-out-confirm"]'],
};
const REUSED: Scene = { setup: { first: both(signed({ source: "reused" })) } };
const STOP_ASK: Scene = { ...REUSED, press: ['[data-testid="plans-stop-using"]'] };
/** Claude signed out with nothing found, ChatGPT with its login found from Codex. */
const SIGNED_OUT: Scene = {
  setup: {
    first: both(
      { signed_out: {} },
      { login_found: { from: "codex", email: "r.mehra@mehrachambers.in" } },
    ),
  },
};
/** The card with no size known: a download of no bytes. */
const ADD0: Scene = {
  setup: {
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, signed()),
      install(CODEX, { missing: { download_bytes: 0 } }),
    ],
  },
};
/** A fresh computer: neither plan here yet, each with its card and Add, Claude's then ChatGPT's. */
const ADD: Scene = {
  setup: {
    first: [
      install(CLAUDE, { missing: { download_bytes: 130_000_000 } }),
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, { missing: { download_bytes: 165_000_000 } }),
    ],
  },
};
/** Both plans here: Claude signed out, ChatGPT's sign-in cancelled. The same card for each, its own key help in the
 * key panel under it, opened from each plan's "Use an API key instead", and "Use Claude instead" since Claude is here. */
const CANCELLED: Scene = {
  setup: { first: both({ signed_out: {} }, { failed: { code: "signin.cancelled", at: AT } }) },
  press: [
    '[data-agent="claude"] [data-testid="plans-key-open"]',
    '[data-agent="chatgpt"] [data-testid="plans-key-open"]',
  ],
};
const KEY: Scene = {
  setup: { first: both({ signed_in: { method: "api_key", source: "own", at: AT } }) },
};
const KEY_REMOVE: Scene = { ...KEY, press: ['[data-testid="plans-key-remove"]'] };
/** ChatGPT's key panel, opened from its "Use an API key instead". */
const CHATGPT_KEY: Scene = {
  ...SIGNED_OUT,
  press: ['[data-agent="chatgpt"] [data-testid="plans-key-open"]'],
};
const found = (f: string): Scene => ({
  setup: { first: both({ signed_out: { found: f as never } }) },
});

const entry = (id: string, key: SettingsKey | WordsRef, scene: Scene): Entry => ({
  id,
  words: typeof key === "string" ? settingsWords(key) : key,
  screen: "settings/plans",
  width: WINDOW.width,
  height: WINDOW.height,
  render: (lang) => {
    const over = <SettingsOver section="plans" lang={lang} setup={scene.setup} />;
    return scene.press === undefined ? (
      over
    ) : (
      <PressInOrder press={scene.press}>{over}</PressInOrder>
    );
  },
});

/** Plans' entries, UD2-32 to UD2-63 and UC-140, each id written as it is. UD2-01 to UD2-31 are the room's (part A). */
export const UD2_PLANS: Entry[] = [
  entry("UD2-32", "Plans", OWN),
  entry(
    "UD2-33",
    "The plans Clauseo works with. Each signs in through its own page in your browser.",
    OWN,
  ),
  entry("UD2-34", "Sign out", OWN),
  entry("UD2-35", "Sign out of {name}?", SIGN_OUT_ASK),
  entry(
    "UD2-36",
    "Your matters and conversations stay on this computer. Questions wait until you sign in again.",
    SIGN_OUT_ASK,
  ),
  entry("UD2-37", "Signed out of {name}.", SIGNED_OUT_HERE),
  entry("UD2-38", "Stop using it here", REUSED),
  entry("UD2-39", "Stop using {app}'s sign-in here?", STOP_ASK),
  entry(
    "UD2-40",
    "This doesn't sign you out of {app}. Clauseo stops using its sign-in, and you can sign in here instead.",
    STOP_ASK,
  ),
  entry("UD2-41", "Look for my {app} sign-in", SIGNED_OUT),
  entry("UD2-42", "Uses your ChatGPT plan.", ADD),
  entry("UD2-43", "Add ChatGPT", ADD),
  entry("UD2-44", "Needs a download of about {mb} MB. Signing in happens in your browser.", ADD),
  // UD2-45, retired: the key is one quiet "Use an API key instead" under each plan (UC-141)
  entry("UD2-46", "Change key", KEY),
  entry("UD2-47", "Remove key", KEY),
  entry("UD2-48", "Remove the API key?", KEY_REMOVE),
  entry("UD2-49", "Questions wait until you sign in or add a key again.", KEY_REMOVE),
  // "again" only after a real sign-in (D-066): Claude signed out from here
  entry("UD2-50", "Sign in again", SIGNED_OUT_HERE),
  // UD2-51 to UD2-54, retired: the firm's address is set by its policy only (plans.<id>), with no field in Plans
  entry("UD2-55", "No {app} sign-in on this computer.", found("none_found")),
  entry("UD2-56", "Clauseo isn't using {app}'s sign-in here.", found("reuse_off")),
  // UD2-59: the card's line with no size known (a download of no bytes)
  entry("UD2-59", "Signing in happens in your browser.", ADD0),
  // UD2-60 to UD2-62: Claude's card, the same as ChatGPT's, when Claude isn't added yet (the plans side by side)
  entry("UD2-60", "Uses your Claude plan.", ADD),
  entry("UD2-61", "Add Claude", ADD),
  entry("UD2-62", "Needs a download of about {mb} MB. Signing in happens in your browser.", ADD),
  // UD2-63, retired: each plan's key help names the key's maker (UC-145, UC-146)
  // UC-140 and UC-146: ChatGPT's key field and help name its maker, UC-146 on both cards' key panels, each with its
  // own (UC-139, UC-145, Claude's, and UC-141 are shown in the room, registry-ud1.tsx)
  entry("UC-140", firstLaunchWords("OpenAI API key"), CHATGPT_KEY),
  entry(
    "UC-146",
    firstLaunchWords("A key from the OpenAI platform. Billed to that account."),
    CANCELLED,
  ),
];
