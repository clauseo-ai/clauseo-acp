// UD1, Done-when 1: each sign-in state in the composer where "Sign in" would be, and each button sending the command
// the brief names; "Sign in" pressed early showing the queued line; the nudge only when the host says so; the paste
// field clearing, with the code absent from the store; LoginFound's line and its two buttons, with nothing reused
// before "Use it"; a Reused sign-in with no step and the through line; each `found` value's plan-found screen; the
// device-code card and its refusal; no key field when keys_available is false; ChatGPT and back.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { setupStoreOf, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FoundLogin, InstallProgress, SignIn } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  CODEX,
  READY,
  SIGNED_IN,
  button,
  byId,
  counts,
  enter,
  info,
  install,
  mountFirstRun,
  play,
  press,
  setups,
  signIn,
  stubClipboard,
  text,
  typeInto,
} from "./rig.tsx";

const OFFLINE: InstallProgress = { waiting: { code: "agent.install_offline", hosts: [], log: [] } };

/** The room with matter A open, Claude installed, and this sign-in state. */
const at = (state: SignIn, more: Parameters<typeof info>[0] = true) =>
  mountFirstRun({ first: [info(more), install(CLAUDE, READY), signIn(CLAUDE, state)] });

const gateState = () => byId("signin-state")?.dataset["state"];
const field = () => byId<HTMLTextAreaElement>("composer");

test("plan: the composer says Sign in to Claude, and Sign in sends StartSignIn; never a model's name", async () => {
  const t = await at({ signed_out: {} });
  expect(byId("signin")?.dataset["agent"]).toBe("claude");
  expect(gateState()).toBe("signed-out");
  expect(text()).toContain("Sign in to Claude to ask about this matter");
  expect(text()).toContain(
    "Your papers stay on this computer. Claude sees what each question sends it.",
  );
  expect(text()).not.toMatch(/Opus|Sonnet|Haiku|GPT-\d|o\d-/);
  // the gate names the plan; the status line above it says nothing of one
  expect(text()).not.toContain("Uses your");
  expect(byId("signin")?.querySelector("img, svg[aria-label]")).toBeNull();
  await press("Sign in");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("plan-early, plan-queued: Sign in works before the install is ready, and the queued line says the browser opens when Claude is ready (offline: when you reconnect)", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 1, total: 200_000_000 } }),
      signIn(CLAUDE, { signed_out: {} }),
    ],
  });
  await press("Sign in");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  await play(t, signIn(CLAUDE, "queued"));
  expect(gateState()).toBe("queued");
  expect(text()).toContain("Your browser opens when Claude is ready.");
  expect(text()).not.toMatch(/Sign in at|\d:\d\d/);
  await play(t, install(CLAUDE, OFFLINE));
  expect(text()).toContain("Your browser opens when you reconnect.");
  await press(byId("signin-cancel"));
  expect(setups(t).at(-1)).toEqual({ cancel_sign_in: { agent: CLAUDE } });
});

test("plan-chatgpt: Use ChatGPT instead sends InstallAgent for codex-acp, and Use Claude instead sends it for claude-acp", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 1, total: 2 } }),
      signIn(CLAUDE, { signed_out: {} }),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  await press("Use ChatGPT instead");
  expect(setups(t)).toEqual([{ install_agent: { agent: CODEX } }]);
  await press("Use Claude instead");
  expect(setups(t)).toEqual([
    { install_agent: { agent: CODEX } },
    { install_agent: { agent: CLAUDE } },
  ]);
});

test("plan-key: the key field is a password field, Use this key sends SetAgentKey and clears it; the Keychain line on the Mac; Console signs in with console", async () => {
  const t = await at({ signed_out: {} });
  await press("Use an API key or my firm's account");
  expect(gateState()).toBe("key");
  const key = byId<HTMLInputElement>("signin-key");
  expect(key?.type).toBe("password");
  expect(text()).toContain("Kept in this Mac's Keychain.");
  await typeInto(key as HTMLInputElement, "sk-test-secret");
  await press("Use this key");
  expect(setups(t)).toEqual([{ set_agent_key: { agent: CLAUDE, key: "sk-test-secret" } }]);
  expect(byId<HTMLInputElement>("signin-key")?.value).toBe("");
  expect(
    JSON.stringify(setupStoreOf(t as unknown as HostTransport & SetupSource).state()),
  ).not.toContain("sk-test-secret");
  await press("Sign in with an Anthropic Console account");
  expect(setups(t).at(-1)).toEqual({ start_sign_in: { agent: CLAUDE, console: true } });
});

test("plan-key: Windows keeps the key in Credential Manager", async () => {
  await mountFirstRun({
    platform: "windows",
    first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  await press("Use an API key or my firm's account");
  expect(text()).toContain("Kept in Windows Credential Manager.");
});

test("plan-key: with keys_available false there is no key field, and the screen says there is no keyring", async () => {
  await at({ signed_out: {} }, false);
  await press("Use an API key or my firm's account");
  expect(byId("signin-key")).toBeNull();
  expect(document.querySelector('input[type="password"]')).toBeNull();
  expect(text()).toContain("This computer has no keyring to keep an API key in.");
});

test("signin-wait: Finish signing in in your browser, with Cancel sending CancelSignIn; no nudge until the host says so", async () => {
  const t = await at({ waiting: { since: AT, nudge: false } });
  expect(gateState()).toBe("waiting");
  expect(text()).toContain("Finish signing in in your browser");
  expect(text()).not.toContain("Didn't open?");
  // the window keeps no clock of its own: time passing brings no nudge
  await new Promise((resolve) => setTimeout(resolve, 40));
  expect(text()).not.toContain("Didn't open?");
  await press(byId("signin-cancel"));
  expect(setups(t)).toEqual([{ cancel_sign_in: { agent: CLAUDE } }]);
});

test("signin-nudge: Didn't open? Try again starts a new StartSignIn; Got a code instead? Paste it opens the paste field", async () => {
  const t = await at({ waiting: { since: AT, nudge: true } });
  expect(text()).toContain("Didn't open?");
  expect(text()).toContain("Got a code instead?");
  await press("Try again");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  await play(t, signIn(CLAUDE, { waiting: { since: AT, nudge: true } }));
  await press(byId("signin-paste"));
  expect(byId("signin-code")).not.toBeNull();
});

test("signin-code: the code goes in PasteSignInCode, the field clears, and the code is in no store", async () => {
  const t = await at({ waiting: { since: AT, nudge: true } });
  await press(byId("signin-paste"));
  expect(text()).toContain("Clauseo passes the code to Claude and keeps no copy.");
  const code = byId<HTMLInputElement>("signin-code") as HTMLInputElement;
  expect(code.type).toBe("text");
  await typeInto(code, "pasted-code-4821");
  await enter(code);
  expect(setups(t)).toEqual([{ paste_sign_in_code: { agent: CLAUDE, code: "pasted-code-4821" } }]);
  expect(byId<HTMLInputElement>("signin-code")?.value ?? "").toBe("");
  const store = setupStoreOf(t as unknown as HostTransport & SetupSource);
  expect(JSON.stringify(store.state())).not.toContain("pasted-code-4821");
  // and nothing the composer keeps: no draft, no field
  expect(field()?.value ?? "").not.toContain("pasted-code-4821");
  // the Enter in the code field asked nothing of the conversation
  expect(t.only("dispatch").map((d) => Object.keys(d.command.command)[0])).not.toContain(
    "send_prompt",
  );
});

test("signin-nudge (ChatGPT): Codex has no paste route; Use a code instead sends UseDeviceCode", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      install(CODEX, READY),
      signIn(CLAUDE, { signed_out: {} }),
      signIn(CODEX, { waiting: { since: AT, nudge: true } }),
    ],
  });
  await press("Use ChatGPT instead");
  expect(byId("signin")?.dataset["agent"]).toBe("chatgpt");
  expect(byId("signin-paste")).toBeNull();
  await press("Use a code instead");
  expect(setups(t).at(-1)).toEqual({ use_device_code: { agent: CODEX } });
});

test("signin-device: the code large, Open page sends OpenSignInPage, Copy copies it, and the time it works until", async () => {
  const writeText = stubClipboard();
  const t = await mountFirstRun({
    first: [
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, {
        device_code: {
          url: "https://auth.openai.com/codex/device",
          code: "ABCD-1234",
          expires_at: AT,
        },
      }),
    ],
  });
  await press("Use ChatGPT instead");
  expect(byId("signin-device-code")?.textContent).toBe("ABCD-1234");
  expect(text()).toContain("Sign in to ChatGPT on that page and enter this code.");
  expect(text()).toContain("The code works until 11:02 am.");
  await press("Open page");
  expect(setups(t).at(-1)).toEqual({ open_sign_in_page: { agent: CODEX } });
  await press("Copy");
  expect(writeText).toHaveBeenCalledWith("ABCD-1234");
});

test("signin-device-off: the refusal says how to turn it on, Try again sends UseDeviceCode, and the browser way StartSignIn", async () => {
  const t = await mountFirstRun({
    first: [
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, { failed: { code: "signin.device_code_off", at: AT } }),
    ],
  });
  await press("Use ChatGPT instead");
  expect(gateState()).toBe("signin.device_code_off");
  expect(text()).toContain("Sign-in with a code is turned off for this ChatGPT account.");
  expect(text()).toContain("Enable device code sign-in for Codex, Excel, PowerPoint, and Word");
  await press("Try again");
  expect(setups(t).at(-1)).toEqual({ use_device_code: { agent: CODEX } });
  await press("Sign in in the browser instead");
  expect(setups(t).at(-1)).toEqual({ start_sign_in: { agent: CODEX } });
});

test("signin-device is never the first offer: the plan and the waiting screen offer no device code", async () => {
  await mountFirstRun({
    first: [
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  await press("Use ChatGPT instead");
  expect(text()).not.toContain("Use a code instead");
});

test("signin-done: Signed in as the email and plan, and the composer takes questions again", async () => {
  const t = await at({ signed_out: {} });
  await play(t, signIn(CLAUDE, SIGNED_IN));
  expect(byId("signin")).toBeNull();
  expect(byId("signin-done")?.textContent).toBe("Signed in as test@example.com · Claude Max");
  expect(field()?.hidden).toBe(false);
  expect(counts(t).filter((c) => c.onboarding_step?.step === "signed_in")).toHaveLength(1);
});

test("signin-done: with a key, Using an API key; with no email or plan, the line without them", async () => {
  const t = await at({ signed_out: {} });
  await play(t, signIn(CLAUDE, { signed_in: { method: "api_key", source: "own", at: AT } }));
  expect(byId("signin-done")?.textContent).toBe("Using an API key, billed to its own account");
  await play(t, signIn(CLAUDE, { signed_in: { method: "plan", source: "own", at: AT } }));
  expect(byId("signin-done")?.textContent).toBe("Signed in to Claude");
});

test("signin-cancel: Sign-in was cancelled in the browser, with Sign in", async () => {
  const t = await at({ failed: { code: "signin.cancelled", at: AT } });
  expect(text()).toContain("Sign-in was cancelled in the browser.");
  await press("Sign in");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("signin-unfinished: the link expired at the host's time, with Try again", async () => {
  const t = await at({ failed: { code: "signin.link_expired", at: AT } });
  expect(text()).toContain("The sign-in link expired at 11:02 am.");
  await press("Try again");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("signin-noplan: no plan, what Clauseo works with, See Claude's plans, Use another account and Use ChatGPT instead", async () => {
  const t = await at({ failed: { code: "signin.no_plan", at: AT } });
  expect(text()).toContain("Clauseo works with a Claude Pro or Max plan.");
  await press("See Claude's plans");
  expect(t.only("open_external").map((c) => c.url)).toEqual(["https://claude.com/pricing"]);
  await press("Use another account");
  expect(setups(t).at(-1)).toEqual({ start_sign_in: { agent: CLAUDE } });
  await press("Use ChatGPT instead");
  expect(setups(t).at(-1)).toEqual({ install_agent: { agent: CODEX } });
});

test("signin-port-busy: Try again sends StartSignIn and Use a code instead UseDeviceCode", async () => {
  const t = await mountFirstRun({
    first: [
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, { failed: { code: "signin.port_busy", at: AT } }),
    ],
  });
  await press("Use ChatGPT instead");
  expect(text()).toContain("Another program on this computer is using ChatGPT's sign-in.");
  await press("Try again");
  expect(setups(t).at(-1)).toEqual({ start_sign_in: { agent: CODEX } });
  await press("Use a code instead");
  expect(setups(t).at(-1)).toEqual({ use_device_code: { agent: CODEX } });
});

test("signin-failed: Sign-in didn't finish, with Try again", async () => {
  const t = await at({ failed: { code: "signin.failed", at: AT } });
  expect(text()).toContain("Sign-in didn't finish.");
  await press("Try again");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("signin-policy: the firm's settings can't be read, Ask your IT team, and no button", async () => {
  await at({ failed: { code: "signin.policy_unreadable", at: AT } });
  expect(text()).toContain(
    "Clauseo can't read your firm's settings for Claude, so it won't sign in until they can be read.",
  );
  expect(byId("signin-state")?.querySelectorAll("button")).toHaveLength(0);
});

test("each sign-in code's screen counts error_shown with its code", async () => {
  const t = await at({ failed: { code: "signin.cancelled", at: AT } });
  await play(t, signIn(CLAUDE, { failed: { code: "signin.no_plan", at: AT } }));
  expect(counts(t).filter((c) => c.error_shown !== undefined)).toEqual([
    { error_shown: { code: "signin.cancelled" } },
    { error_shown: { code: "signin.no_plan" } },
  ]);
});

test("login-found: the account line and its two buttons, each sending its command, and nothing reused before Use it", async () => {
  const t = await at({ login_found: { from: "claude_code", email: "test@example.com" } });
  expect(byId("login-found")?.textContent).toBe(
    "Clauseo found your Claude account (test@example.com) from Claude Code and will use it here.",
  );
  expect(text()).toContain(
    "A different account signs in to Clauseo only. Claude Code stays as it is.",
  );
  // nothing reuses before Use it: no command at all, and the composer still holds questions back
  expect(setups(t)).toEqual([]);
  expect(field()?.hidden).toBe(true);
  await press(byId("login-use"));
  expect(setups(t)).toEqual([{ use_found_login: { agent: CLAUDE } }]);
  await press(byId("login-other"));
  expect(setups(t).at(-1)).toEqual({ use_different_account: { agent: CLAUDE } });
  // after Use it, the through line
  await play(
    t,
    signIn(CLAUDE, {
      signed_in: {
        email: "test@example.com",
        plan: "Claude Max",
        method: "plan",
        source: "reused",
        at: AT,
      },
    }),
  );
  expect(byId("signin-done")?.textContent).toBe(
    "Signed in through Claude Code as test@example.com · Claude Max",
  );
});

test("login-found (ChatGPT, no email): the line without the email, from Codex", async () => {
  await mountFirstRun({
    first: [
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, { login_found: { from: "codex" } }),
    ],
  });
  await press("Use ChatGPT instead");
  expect(byId("login-found")?.textContent).toBe(
    "Clauseo found your ChatGPT account from Codex and will use it here.",
  );
  expect(text()).toContain("Codex stays as it is.");
});

test("a Reused sign-in at a later start shows no sign-in step, and the through line", async () => {
  const t = await at({
    signed_in: {
      email: "test@example.com",
      plan: "Claude Max",
      method: "plan",
      source: "reused",
      at: AT,
    },
  });
  expect(byId("signin")).toBeNull();
  expect(field()?.hidden).toBe(false);
  expect(byId("signin-done")?.textContent).toBe(
    "Signed in through Claude Code as test@example.com · Claude Max",
  );
  expect(setups(t)).toEqual([]);
});

const FOUND: [FoundLogin, string][] = [
  [
    "claude_settings",
    "Claude Code on this computer signs in through your own Claude settings, which Clauseo keeps out of your matters.",
  ],
  [
    "claude_windows_sandbox",
    "Clauseo's safe workspace can't share Claude Code's sign-in on Windows yet.",
  ],
  [
    "reused_signed_out",
    "Claude Code on this computer is signed out, so Clauseo stopped using its sign-in.",
  ],
  [
    "reused_changed",
    "Claude Code on this computer now uses another account, so Clauseo stopped using its sign-in.",
  ],
  ["reused_failed", "Claude Code's sign-in stopped working here, so Clauseo stopped using it."],
];

test.each<FoundLogin>(["none_found", "reuse_off"])(
  "plan (%s): no login found and reuse off are the plain plan screen, with no found line",
  async (found) => {
    const t = await at({ signed_out: { found } });
    for (const [, line] of FOUND) expect(text()).not.toContain(line);
    await press("Sign in");
    expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  },
);

test.each(FOUND)(
  "plan-found (%s): its line, Sign in sending StartSignIn, and the key or firm's account for Claude",
  async (found, line) => {
    const t = await at({ signed_out: { found } });
    expect(text()).toContain(line);
    await press("Sign in");
    expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
    await press("Use an API key or my firm's account");
    expect(byId("signin-key")).not.toBeNull();
  },
);

test.each<[FoundLogin, string]>([
  ["codex_keyring", "Codex on this computer keeps its sign-in where Clauseo can't share it."],
  ["codex_link_split", "Codex's sign-in on this computer changed, so Clauseo stopped using it."],
  [
    "reused_signed_out",
    "Codex on this computer is signed out, so Clauseo stopped using its sign-in.",
  ],
])("plan-found (ChatGPT, %s): its line and Sign in, with no key offer", async (found, line) => {
  const t = await mountFirstRun({
    first: [
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, { signed_out: { found } }),
    ],
  });
  await press("Use ChatGPT instead");
  expect(text()).toContain(line);
  expect(text()).not.toContain("Use an API key or my firm's account");
  await press("Sign in");
  expect(setups(t).at(-1)).toEqual({ start_sign_in: { agent: CODEX } });
});

test("plan-key: opening the key panel puts the focus in the API key field, and Cancel gives it back to the button that opened it", async () => {
  await at({ signed_out: {} });
  const opener = button("Use an API key or my firm's account") as HTMLButtonElement;
  opener.focus();
  await press(opener);
  expect(document.activeElement).toBe(byId("signin-key"));
  await press(byId("signin")?.querySelector<HTMLButtonElement>(".fl-gate-cancel") ?? null);
  expect(byId("signin-key")).toBeNull();
  expect(document.activeElement?.textContent).toBe("Use an API key or my firm's account");
});

test("signin-nudge: Paste it puts the focus in the code field, and Cancel gives it back to Paste it", async () => {
  await at({ waiting: { since: AT, nudge: true } });
  byId("signin-paste")?.focus();
  await press(byId("signin-paste"));
  expect(document.activeElement).toBe(byId("signin-code"));
  await press(byId("signin-cancel"));
  expect(byId("signin-code")).toBeNull();
  expect(document.activeElement).toBe(byId("signin-paste"));
});
