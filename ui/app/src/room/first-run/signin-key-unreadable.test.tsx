// UD1, the gate for a saved key the Mac's keychain didn't give (agent.key_unreadable, decision record 0048): the
// key's own words, never "Sign-in didn't finish.", and Try again, which reads the key again (desktop refuses the
// sign-in start with agent.key_unreadable and re-reads it). Kept apart from signin.test.tsx for its 600-line limit.
// @vitest-environment happy-dom
import { beforeAll, expect, test } from "vitest";
import type { HostError, SignIn } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  CODEX,
  READY,
  allById,
  byId,
  info,
  install,
  mountFirstRun,
  play,
  press,
  settle,
  settingsView,
  setups,
  signIn,
  text,
} from "./rig.tsx";

// The first mount loads the room's lazy chunks, which takes seconds on a busy machine: they load here, under the
// hook's own 30 s, so the test's 5 s limit is not spent on them (as signin.test.tsx does).
beforeAll(async () => {
  await Promise.all([
    import("../first-read/first-read-offer.tsx"),
    import("../new-matter.tsx"),
    import("../locate-warning.tsx"),
    import("../thread/list.tsx"),
    import("../protection/note-load.ts").then((m) => m.loadProtectionNote()),
    import("../signin-expired/expired-card.tsx"),
  ]);
}, 30_000);

const refusal = (code: string): HostError => ({
  code: code as never,
  message: "refused",
  retryable: false,
});

/** The room with matter A open, Claude installed, and this sign-in state; `refuse` refuses every sign-in start. */
const at = (state: SignIn, refuse?: string) =>
  mountFirstRun({
    first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, state)],
    command: (c) => {
      if (refuse !== undefined && c.start_sign_in !== undefined) throw refusal(refuse);
    },
  });
const KEY_WORDS = "Clauseo couldn't check for a saved API key in time.";
const UNREADABLE: SignIn = { failed: { code: "agent.key_unreadable", at: AT } };

test("key-unreadable: the saved key in its own words, never Sign-in didn't finish, and Try again", async () => {
  const t = await at({ failed: { code: "agent.key_unreadable", at: AT } });
  expect(text()).toContain("Clauseo couldn't check for a saved API key in time.");
  expect(text()).not.toContain("Sign-in didn't finish.");
  await press("Try again");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("key-unreadable: the gate also says where the key can be changed, Settings then Plans", async () => {
  await at(UNREADABLE);
  expect(text()).toContain(
    `${KEY_WORDS} If this computer asks for permission, allow it, then try again.`,
  );
  expect(byId("signin-state")?.textContent).toContain("To change it, open Settings, then Plans.");
});

test("key-unreadable: Try again refused in the same code adds no second line under the gate", async () => {
  const t = await at(UNREADABLE, "agent.key_unreadable");
  await press("Try again");
  await settle();
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(byId("signin-refused")).toBeNull();
  // the gate (its refused line included) says the key's words once
  expect(byId("signin-state")?.textContent?.split(KEY_WORDS)).toHaveLength(2);
});

test("key-unreadable: Try again refused in another code says that code's line under the gate", async () => {
  const other = await at(UNREADABLE, "signin.key_in_use");
  await press("Try again");
  await settle();
  expect(setups(other)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(byId("signin-refused")?.textContent).toContain("Clauseo is using your API key.");
});

// VM rows A-K6 and D7 on 4d2d29744: the start window kept "Claude is ready" with Sign in with
// Claude after the keychain's bound passed, while Plans and the thread said the key's words.
// Desktop sends the key's state late (past the 10 s bound, once the window has drawn), through
// the same setup events the store and the first screen follow.
test("key-unreadable on the start window, late: the key's words, the Plans line and Try again, never a sign-in", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [info(true)],
    command: (c) => {
      if (c.start_sign_in !== undefined) throw refusal("agent.key_unreadable");
    },
  });
  // the keys still load: desktop has sent no plan's state; each plan shows the sign-in check's line, Sign in busy
  expect(allById("plan-row").map((r) => r.getAttribute("data-agent"))).toEqual([CLAUDE, CODEX]);
  expect(text()).toContain("Checking whether you're signed in to Claude…");
  expect(text()).toContain("Checking whether you're signed in to ChatGPT…");
  expect(text()).not.toContain("Sign in with the plan you pay for");
  // the bound passes: the install and the signed-out plan come first, the start window draws Claude's row
  await play(t, install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} }));
  expect(byId("signin-foot-sign-in")?.textContent).toBe("Sign in with Claude");
  // then the key's late answer: unreadable
  await play(t, signIn(CLAUDE, UNREADABLE));
  const row = document.querySelector(`[data-testid="plan-row"][data-agent="${CLAUDE}"]`);
  expect(byId("signin-foot")?.dataset["state"]).toBe("key-unreadable");
  expect(row?.textContent).toContain(KEY_WORDS);
  expect(row?.textContent).toContain("To change it, open Settings, then Plans.");
  expect(row?.querySelector("[data-lead]")?.getAttribute("data-lead")).toBe("none");
  expect(text()).not.toContain("is ready");
  expect(text()).not.toContain("Sign in with Claude");
  expect(text()).not.toContain("Sign in with the plan you pay for");
  expect(byId("install-said")?.textContent).toContain(KEY_WORDS);
  const again = byId("signin-foot-sign-in");
  expect(again?.textContent).toBe("Try again");
  // Try again: desktop refuses it with the key's code and reads the key again; the row stays the key's
  await press(again);
  await settle();
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(byId("signin-foot")?.dataset["state"]).toBe("key-unreadable");
  expect(text()).not.toContain("Sign in with Claude");
});

// The Mac mini run of e6c3fb85b: after Remove key in Plans, desktop's detection found Claude Code's login, and the
// start window drew no Claude row (login_found had no line there); it shows the found line with Use it
test("key-denied on the start window, then Remove key: Claude's row comes back with the found login's Use it", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      info(true),
      install(CLAUDE, READY),
      signIn(CLAUDE, { failed: { code: "agent.key_denied", at: AT } }),
    ],
  });
  expect(byId("signin-foot")?.dataset["state"]).toBe("key-unreadable");
  await play(t, signIn(CLAUDE, { login_found: { from: "claude_code" } }));
  const row = document.querySelector(`[data-testid="plan-row"][data-agent="${CLAUDE}"]`);
  expect(row?.textContent).toContain(
    "Clauseo found your Claude account from Claude Code and will use it here.",
  );
  expect(text()).not.toContain("couldn't read the saved API key");
  await press(byId("signin-foot-use"));
  expect(setups(t)).toEqual([{ use_found_login: { agent: CLAUDE } }]);
});

// The review of 37eaf3f1b: a plan the firm locks has no row, and its install finishing in this
// window set the "{name} is ready" tick (readyHere) whatever its key; an unreadable key never does
test("key-unreadable on the start window, a plan the firm locks: no ready tick after its install finished here", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [info(true), install(CLAUDE, { downloading: { received: 1, total: 2 } })],
    settings: settingsView({ locked: ["plans.claude-acp"] }),
  });
  await play(t, install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} }));
  await play(t, signIn(CLAUDE, UNREADABLE));
  expect(text()).not.toContain("Claude is ready");
  expect(text()).not.toContain("Sign in with Claude");
  expect(allById("plan-row").filter((r) => r.getAttribute("data-agent") === CLAUDE)).toEqual([]);
  // control: the key read, the plan signed in by it, the tick shows
  await play(t, signIn(CLAUDE, { signed_in: { method: "api_key", source: "own", at: AT } }));
  expect(text()).toContain("Claude is ready");
});

test("key-unreadable on the start window: Try again refused in another code says that code's words", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, UNREADABLE)],
    command: (c) => {
      if (c.start_sign_in !== undefined) throw refusal("command.conflict");
    },
  });
  const conflict = "That request clashes with one already made, so nothing changed.";
  expect(text()).not.toContain(conflict);
  await press(byId("signin-foot-sign-in"));
  await settle();
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  const row = document.querySelector(`[data-testid="plan-row"][data-agent="${CLAUDE}"]`);
  expect(row?.textContent).toContain(conflict);
  expect(row?.textContent).toContain("To change it, open Settings, then Plans.");
  expect(byId("install-said")?.textContent).toContain(conflict);
  expect(byId("signin-foot")?.dataset["state"]).toBe("key-unreadable");
});

// The real-Mac run of 49042948c: after a Deny (keyring's PlatformFailure) the gate and the start window said "in time",
// which was untrue. agent.key_denied has its own line, in the same places, with the same Plans line and Try again.
const DENIED_WORDS =
  "Clauseo couldn't read the saved API key on this computer. Try again, and if this computer asks for permission, choose Allow. If it keeps happening, show this to whoever looks after this computer.";
const DENIED: SignIn = { failed: { code: "agent.key_denied", at: AT } };

test("key-denied: the gate in its own words, never in time, the Plans line, and Try again said once", async () => {
  const t = await at(DENIED, "agent.key_denied");
  const gate = byId("signin-state")?.textContent ?? "";
  expect(gate).toContain(DENIED_WORDS);
  expect(gate).not.toContain(KEY_WORDS);
  expect(gate).toContain("To change it, open Settings, then Plans.");
  expect(text()).not.toContain("Sign-in didn't finish.");
  await press("Try again");
  await settle();
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(byId("signin-refused")).toBeNull();
  expect(byId("signin-state")?.textContent?.split(DENIED_WORDS)).toHaveLength(2);
});

test("key-denied on the start window: its own words, the Plans line and Try again, never a sign-in or the tick", async () => {
  await mountFirstRun({
    matters: [],
    first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, DENIED)],
  });
  const row = document.querySelector(`[data-testid="plan-row"][data-agent="${CLAUDE}"]`);
  expect(byId("signin-foot")?.dataset["state"]).toBe("key-unreadable");
  expect(row?.textContent).toContain(DENIED_WORDS);
  expect(row?.textContent).not.toContain(KEY_WORDS);
  expect(row?.textContent).toContain("To change it, open Settings, then Plans.");
  expect(byId("install-said")?.textContent).toContain(DENIED_WORDS);
  expect(text()).not.toContain("is ready");
  expect(text()).not.toContain("Sign in with Claude");
  expect(text()).not.toContain("Sign in with the plan you pay for");
  expect(byId("signin-foot-sign-in")?.textContent).toBe("Try again");
});

// The real-Mac rerun of A-K6 (e): for the 10 s the keys' load took, desktop sent no plan's state and the start window
// stood empty. Each plan now shows the sign-in check's existing line, Sign in busy, until the first state comes.
test("the keys still loading: both plans show the sign-in check's line with Sign in busy, then their own rows", async () => {
  const t = await mountFirstRun({ matters: [], first: [info(true)] });
  const rows = allById("plan-row");
  expect(rows.map((r) => r.getAttribute("data-agent"))).toEqual([CLAUDE, CODEX]);
  for (const [r, name] of [
    [rows[0], "Claude"],
    [rows[1], "ChatGPT"],
  ] as const) {
    expect(r?.textContent).toContain(`Checking whether you're signed in to ${name}…`);
    const sign = r?.querySelector<HTMLButtonElement>("button");
    expect(sign?.textContent).toBe(`Sign in with ${name}`);
    expect(sign?.disabled).toBe(true);
    expect(sign?.getAttribute("aria-busy")).toBe("true");
  }
  expect(text()).not.toContain("is ready");
  expect(text()).not.toContain("Sign in with the plan you pay for");
  // the load ends: each plan's own row
  await play(
    t,
    install(CLAUDE, READY),
    signIn(CLAUDE, UNREADABLE),
    install(CODEX, READY),
    signIn(CODEX, { signed_out: {} }),
  );
  expect(text()).not.toContain("Checking whether you're signed in");
  expect(byId("signin-foot")?.dataset["state"]).toBe("key-unreadable");
  expect(setups(t)).toEqual([]);
});

// (g): Try again on the key's row picked Claude as the plan in play, so the papers line dropped "or ChatGPT", while
// ChatGPT still offered its own sign-in. Try again picks no plan; the lead and the line keep both.
test("key-unreadable's Try again picks no plan: the lead and the papers line still name ChatGPT, which offers its sign-in", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      info(true),
      install(CLAUDE, READY),
      signIn(CLAUDE, UNREADABLE),
      install(CODEX, READY),
      signIn(CODEX, { signed_out: {} }),
    ],
    command: (c) => {
      if (c.start_sign_in !== undefined) throw refusal("agent.key_unreadable");
    },
  });
  const both = "Claude or ChatGPT sees the parts it reads to answer you.";
  const lead = "Sign in with the plan you pay for: Claude, ChatGPT or both.";
  expect(text()).toContain(both);
  expect(text()).toContain(lead);
  const claude = document.querySelector(`[data-testid="plan-row"][data-agent="${CLAUDE}"]`);
  await press(claude?.querySelector<HTMLElement>('[data-testid="signin-foot-sign-in"]') ?? null);
  await settle();
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(text()).toContain(both);
  expect(text()).toContain(lead);
  expect(text()).toContain("Sign in with ChatGPT");
});

// (h): the key's row is a group named by its visible cause, for a screen reader; other rows are no group.
test.each([
  [
    "agent.key_unreadable",
    `${KEY_WORDS} If this computer asks for permission, allow it, then try again.`,
  ],
  ["agent.key_denied", DENIED_WORDS],
])(
  "%s on the start window: the row is a group named by the key's visible line",
  async (code, words) => {
    await mountFirstRun({
      matters: [],
      first: [
        info(true),
        install(CLAUDE, READY),
        signIn(CLAUDE, { failed: { code: code as never, at: AT } }),
        install(CODEX, READY),
        signIn(CODEX, { signed_out: {} }),
      ],
    });
    const row = document.querySelector(`[data-testid="plan-row"][data-agent="${CLAUDE}"]`);
    expect(row?.getAttribute("role")).toBe("group");
    expect(row?.getAttribute("aria-label")).toBe(words);
    expect(row?.textContent).toContain(words);
    const other = document.querySelector(`[data-testid="plan-row"][data-agent="${CODEX}"]`);
    expect(other?.getAttribute("role")).toBeNull();
    expect(other?.getAttribute("aria-label")).toBeNull();
  },
);
