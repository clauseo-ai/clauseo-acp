// @vitest-environment happy-dom
// Plans (UD2), one test per row of the brief's table: the words, the buttons, and the one command each sends with the
// right agent. Sign out, Stop using it here and Remove key each send only after their confirm, and Cancel sends
// nothing; no matter or thread command goes out from Plans. Add ChatGPT installs, then signs in once ready. A locked
// plan shows Managed by firm and no control, and never env_names or provider. The key goes into one command and
// nowhere else.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import { setupStoreOf, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FoundLogin, SignIn } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  CODEX,
  READY,
  block,
  button,
  hostCommands,
  install,
  mountPlans,
  play,
  press,
  settle,
  signIn,
  signed,
  typeInto,
  unmount,
} from "./plans-rig.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

beforeEach(() => localStorage.clear());
afterEach(async () => {
  await unmount();
  vi.useRealTimers();
});

const both = (claude: SignIn, codex: SignIn = { signed_out: {} }) => [
  install(CLAUDE, READY),
  signIn(CLAUDE, claude),
  install(CODEX, READY),
  signIn(CODEX, codex),
];
const at = (claude: SignIn, more: Parameters<typeof mountPlans>[0] = {}) =>
  mountPlans({ first: both(claude), ...more });
const words = (plan: "claude" | "chatgpt" = "claude") => block(plan)?.textContent ?? "";
const NO_HOST_COMMANDS = (t: Awaited<ReturnType<typeof mountPlans>>) =>
  expect(
    hostCommands(t).filter((k) =>
      /matter|thread|prompt|scan|cancel$|restart|retry_from/.test(k ?? ""),
    ),
  ).toEqual([]);

test("Plans is the first section, with its name and lead, one block per plan: Claude, then ChatGPT", async () => {
  await at(signed());
  expect(document.querySelector(".set-pane h2")?.textContent).toBe("Plans");
  expect(document.body.textContent).toContain(
    "The plans Clauseo works with. Each signs in through its own page in your browser.",
  );
  const rows = [...document.querySelectorAll<HTMLElement>('[data-testid="plans-row"]')];
  expect(rows.map((r) => r.dataset["agent"])).toEqual(["claude", "chatgpt"]);
});

test("own login: UD1's signed-in line and Sign out, which asks first; Cancel sends nothing, the confirm sends sign_out and then says Signed out", async () => {
  const t = await at(signed());
  expect(words()).toContain("Signed in as r.mehra@mehrachambers.in · Claude Max");
  expect(button("Stop using it here", block("claude")!)).toBeNull();
  await press(document.querySelector('[data-testid="plans-sign-out"]'));
  expect(document.body.textContent).toContain("Sign out of Claude?");
  expect(document.body.textContent).toContain(
    "Your matters and conversations stay on this computer. Questions wait until you sign in again.",
  );
  await press(document.querySelector('[data-testid="plans-sign-out-cancel"]'));
  expect(t.setups()).toEqual([]);
  await press(document.querySelector('[data-testid="plans-sign-out"]'));
  await press(document.querySelector('[data-testid="plans-sign-out-confirm"]'));
  expect(t.setups()).toEqual([{ sign_out: { agent: CLAUDE } }]);
  await play(t, signIn(CLAUDE, { signed_out: {} }));
  expect(words()).toContain("Signed out of Claude.");
  expect(button("Sign in again", block("claude")!)).not.toBeNull();
  NO_HOST_COMMANDS(t);
});

test("own login without an email or a plan says Signed in to Claude", async () => {
  await at({ signed_in: { method: "plan", source: "own", at: AT } });
  expect(words()).toContain("Signed in to Claude");
});

test("reused login: the through line, no Sign out, and Stop using it here, which sends stop_using_login only after its confirm", async () => {
  const t = await at(signed({ source: "reused" }));
  expect(words()).toContain(
    "Signed in through Claude Code as r.mehra@mehrachambers.in · Claude Max",
  );
  expect(document.querySelector('[data-testid="plans-sign-out"]')).toBeNull();
  await press(document.querySelector('[data-testid="plans-stop-using"]'));
  expect(document.body.textContent).toContain("Stop using Claude Code's sign-in here?");
  expect(document.body.textContent).toContain(
    "This doesn't sign you out of Claude Code. Clauseo stops using its sign-in, and you can sign in here instead.",
  );
  await press(document.querySelector('[data-testid="plans-stop-using-cancel"]'));
  expect(t.setups()).toEqual([]);
  await press(document.querySelector('[data-testid="plans-stop-using"]'));
  await press(document.querySelector('[data-testid="plans-stop-using-confirm"]'));
  expect(t.setups()).toEqual([{ stop_using_login: { agent: CLAUDE } }]);
  // while the leave finishes, #503's Switching lines, and nothing to press
  await play(t, signIn(CLAUDE, { switching: { since: AT, stuck: false } }));
  expect(words()).toContain("Switching accounts.");
  expect(t.setups().some((c) => "sign_out" in c)).toBe(false);
});

test("signed out: Sign in again sends start_sign_in, and Look for my Claude Code sign-in sends find_login, each for its own plan", async () => {
  const t = await at({ signed_out: {} });
  await press(button("Sign in again", block("claude")!));
  await press(block("chatgpt")!.querySelector('[data-testid="plans-find-login"]'));
  expect(words("chatgpt")).toContain("Look for my Codex sign-in");
  await press(block("claude")!.querySelector('[data-testid="plans-find-login"]'));
  expect(t.setups()).toEqual([
    { start_sign_in: { agent: CLAUDE } },
    { find_login: { agent: CODEX } },
    { find_login: { agent: CLAUDE } },
  ]);
});

test("a search that finds nothing, or reuse turned off, says so once the host's word arrives (stage D's FoundLogin)", async () => {
  const t = await at({ signed_out: {} });
  expect(words()).not.toContain("No Claude Code sign-in");
  await play(t, signIn(CLAUDE, { signed_out: { found: "none_found" as FoundLogin } }));
  expect(words()).toContain("No Claude Code sign-in on this computer.");
  await play(t, signIn(CLAUDE, { signed_out: { found: "reuse_off" as FoundLogin } }));
  expect(words()).toContain("Clauseo isn't using Claude Code's sign-in here.");
  expect(block("claude")!.querySelector('[data-testid="plans-find-login"]')).not.toBeNull();
});

test.each<[FoundLogin, string]>([
  [
    "claude_settings",
    "Claude Code on this computer signs in through your own Claude settings, which Clauseo keeps out of your matters.",
  ],
  [
    "reused_signed_out",
    "Claude Code on this computer is signed out, so Clauseo stopped using its sign-in.",
  ],
  ["reused_failed", "Claude Code's sign-in stopped working here, so Clauseo stopped using it."],
])("found %s: UD1's line above Sign in", async (found, line) => {
  const t = await at({ signed_out: { found } });
  expect(words()).toContain(line);
  expect(block("claude")!.querySelector('[data-testid="plans-find-login"]')).toBeNull();
  await press(button("Sign in", block("claude")!));
  const other = button("Sign in with a different account", block("claude")!);
  expect(other !== null).toBe(found === "reused_failed");
  if (other !== null) await press(other);
  expect(t.setups()).toEqual([
    { start_sign_in: { agent: CLAUDE } },
    ...(found === "reused_failed" ? [{ use_different_account: { agent: CLAUDE } }] : []),
  ]);
});

test("login found: UD1's found card, sending use_found_login or use_different_account", async () => {
  const t = await at({ login_found: { from: "claude_code", email: "r.mehra@mehrachambers.in" } });
  expect(words()).toContain(
    "Clauseo found your Claude account (r.mehra@mehrachambers.in) from Claude Code and will use it here.",
  );
  await press(button("Use it", block("claude")!));
  await press(button("Sign in with a different account", block("claude")!));
  expect(t.setups()).toEqual([
    { use_found_login: { agent: CLAUDE } },
    { use_different_account: { agent: CLAUDE } },
  ]);
});

test("signing in: UD1's panel inline, with the paste field and Cancel; a failure shows gate-failed's words; the main window is not asked for", async () => {
  const t = await at({ waiting: { since: AT, nudge: true } });
  expect(words()).toContain("Finish signing in in your browser");
  expect(words()).toContain("Didn't open?");
  await press(button("Paste it", block("claude")!));
  const field = block("claude")!.querySelector<HTMLInputElement>("input");
  expect(field).not.toBeNull();
  await typeInto(field!, "code-from-the-browser");
  await press(button("Send", block("claude")!));
  await press(button("Cancel", block("claude")!));
  expect(t.setups()).toEqual([
    { paste_sign_in_code: { agent: CLAUDE, code: "code-from-the-browser" } },
    { cancel_sign_in: { agent: CLAUDE } },
  ]);
  await play(t, signIn(CLAUDE, { failed: { code: "signin.failed", at: AT } }));
  expect(words()).toContain("Sign-in didn't finish.");
  expect(t.only("open_settings")).toEqual([]);
  await play(
    t,
    signIn(CODEX, { device_code: { url: "https://x", code: "ABCD-1234", expires_at: AT } }),
  );
  expect(words("chatgpt")).toContain("ABCD-1234");
});

test("ChatGPT not added: Add ChatGPT with its size, install_agent, then start_sign_in once ready, and the install lines meanwhile", async () => {
  const t = await mountPlans({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, signed()),
      install(CODEX, { missing: { download_bytes: 165_000_000 } }),
    ],
  });
  expect(words("chatgpt")).toContain("Uses your ChatGPT plan.");
  expect(words("chatgpt")).toContain(
    "Only ChatGPT's part downloads, about 165 MB. Signing in happens in your browser.",
  );
  await press(document.querySelector('[data-testid="plans-add"]'));
  expect(t.setups()).toEqual([{ install_agent: { agent: CODEX } }]);
  await play(t, install(CODEX, { downloading: { received: 10_000_000, total: 165_000_000 } }));
  expect(words("chatgpt")).toContain("Getting ChatGPT ready. About 165 MB.");
  await play(
    t,
    install(CODEX, { failed: { code: "agent.install_failed", hosts: [], log: ["x"] } }),
  );
  await press(button("Try again", block("chatgpt")!));
  await play(t, install(CODEX, READY), signIn(CODEX, { signed_out: {} }));
  expect(t.setups()).toEqual([
    { install_agent: { agent: CODEX } },
    { retry_agent_install: { agent: CODEX } },
    { start_sign_in: { agent: CODEX } },
  ]);
  await play(t, signIn(CODEX, "queued"));
  expect(t.setups().filter((c) => "start_sign_in" in c)).toHaveLength(1);
});

test("a declined install confirm leaves Add ChatGPT as it was, and says nothing changed", async () => {
  const t = await mountPlans({
    first: [install(CODEX, { missing: { download_bytes: 165_000_000 } })],
    command: () => {
      throw { code: "transport.unauthorized", message: "declined", retryable: false };
    },
  });
  await press(document.querySelector('[data-testid="plans-add"]'));
  expect(document.querySelector('[data-testid="plans-add"]')).not.toBeNull();
  expect(words("chatgpt")).toContain("That wasn't allowed, so nothing changed.");
  await play(t, install(CODEX, READY));
  expect(t.setups().filter((c) => "start_sign_in" in c)).toEqual([]);
});

test("the key: Use this key sends set_agent_key with the typed text; the field is empty after, and the key is in no store and not on the page", async () => {
  const t = await at({ signed_out: {} });
  expect(words()).toContain("Other ways in");
  expect(words()).toContain("Kept in this Mac's Keychain.");
  const field = block("claude")!.querySelector<HTMLInputElement>('[data-testid="plans-key"]')!;
  expect(field.type).toBe("password");
  expect(field.autocomplete).toBe("off");
  await typeInto(field, "sk-ant-api03-secret");
  await press(button("Use this key", block("claude")!));
  expect(t.setups()).toEqual([{ set_agent_key: { agent: CLAUDE, key: "sk-ant-api03-secret" } }]);
  expect(field.value).toBe("");
  const store = setupStoreOf(t as HostTransport & SetupSource);
  expect(JSON.stringify(store.state())).not.toContain("sk-ant");
  expect(document.body.textContent).not.toContain("sk-ant");
});

test("API key in use: UD1's line, no Sign in again, Change key with the same panel, and Remove key after its confirm", async () => {
  const t = await at({ signed_in: { method: "api_key", source: "own", at: AT } });
  expect(words()).toContain("Using an API key, billed to its own account");
  expect(button("Sign in again", block("claude")!)).toBeNull();
  expect(document.querySelector('[data-testid="plans-sign-out"]')).toBeNull();
  await press(button("Change key", block("claude")!));
  const field = block("claude")!.querySelector<HTMLInputElement>('[data-testid="plans-key"]')!;
  await typeInto(field, "sk-ant-api03-new");
  await press(button("Use this key", block("claude")!));
  await press(document.querySelector('[data-testid="plans-key-remove"]'));
  expect(document.body.textContent).toContain("Remove the API key?");
  expect(document.body.textContent).toContain(
    "Questions wait until you sign in or add a key again.",
  );
  await press(document.querySelector('[data-testid="plans-key-remove-cancel"]'));
  await press(document.querySelector('[data-testid="plans-key-remove"]'));
  await press(document.querySelector('[data-testid="plans-key-remove-confirm"]'));
  expect(t.setups()).toEqual([
    { set_agent_key: { agent: CLAUDE, key: "sk-ant-api03-new" } },
    { clear_agent_key: { agent: CLAUDE } },
  ]);
  NO_HOST_COMMANDS(t);
});

test("no keyring: no key panel, and UD1's line", async () => {
  await at({ signed_out: {} }, { keys: false });
  expect(block("claude")!.querySelector('[data-testid="plans-key"]')).toBeNull();
  expect(words()).toContain("This computer has no keyring to keep an API key in.");
});

test("firm's address, Claude only: Connect sends set_firm_endpoint with the url, Remove address sends it with none; a refused address keeps what was typed", async () => {
  let refuse = false;
  const t = await at(
    { signed_in: { method: "firm", source: "own", at: AT } },
    {
      plans: { [CLAUDE]: { endpoint: "https://ai.mehrachambers.in" } },
      command: () => {
        if (refuse) throw { code: "settings.refused", message: "bad", retryable: false };
      },
    },
  );
  expect(words()).toContain("Using your firm's account");
  expect(block("chatgpt")!.querySelector('[data-testid="plans-endpoint"]')).toBeNull();
  const field = block("claude")!.querySelector<HTMLInputElement>(
    '[data-testid="plans-endpoint"] input',
  )!;
  expect(field.value).toBe("https://ai.mehrachambers.in");
  await press(button("Remove address", block("claude")!));
  refuse = true;
  await typeInto(field, "http://not-https");
  await press(button("Connect", block("claude")!));
  expect(t.setups()).toEqual([
    { set_firm_endpoint: { agent: CLAUDE } },
    { set_firm_endpoint: { agent: CLAUDE, url: "http://not-https" } },
  ]);
  expect(field.value).toBe("http://not-https");
  expect(block("claude")!.querySelector('[data-testid="plans-refused"]')).not.toBeNull();
});

test.each([["plans.claude-acp"], ["plans."]])(
  "locked by %s: the method line and Managed by firm, no control, and never env_names or provider",
  async (lock) => {
    await at(signed(), {
      locked: [lock],
      plans: {
        [CLAUDE]: {
          endpoint: "https://ai.mehrachambers.in",
          provider: "bedrock" as never,
          env_names: ["SECRET_ENV_NAME" as never],
        },
      },
    });
    const claude = block("claude")!;
    expect(claude.dataset["state"]).toBe("locked");
    expect(claude.querySelector('[data-testid="plans-locked"]')?.textContent).toBe(
      "Managed by firm",
    );
    expect(claude.textContent).toContain("Signed in as r.mehra@mehrachambers.in · Claude Max");
    expect(claude.querySelectorAll("button, input")).toHaveLength(0);
    expect(document.body.textContent).not.toMatch(/SECRET_ENV_NAME|bedrock/);
    expect(block("chatgpt")!.dataset["state"]).toBe(lock === "plans." ? "locked" : "signed-out");
  },
);

test.each([
  ["signin.key_in_use"],
  ["signin.endpoint_in_use"],
  ["signin.firm_managed"],
  ["signin.login_shared"],
  ["settings.key_stored"],
  ["signin.agents_still_running"],
  ["agent.keys_loading"],
  ["transport.unauthorized"],
])("a refused command (%s) shows its code's words under the block", async (code) => {
  const { errorWords } = await import("@clauseo/components/lib/words-errors.ts");
  await at(
    { signed_out: {} },
    {
      command: () => {
        throw { code, message: "refused", retryable: false };
      },
    },
  );
  await press(button("Sign in again", block("claude")!));
  expect(block("claude")!.querySelector('[data-testid="plans-refused"]')?.textContent).toBe(
    errorWords("en", code),
  );
});

test("a command.conflict while switching says Clauseo is still switching accounts, and the typed key stays", async () => {
  const t = await at(
    { switching: { since: AT, stuck: false } },
    {
      command: () => {
        throw { code: "command.conflict", message: "busy", retryable: true };
      },
    },
  );
  const field = block("claude")!.querySelector<HTMLInputElement>('[data-testid="plans-key"]')!;
  await typeInto(field, "sk-ant-api03-kept");
  await press(button("Use this key", block("claude")!));
  expect(words()).toContain("Clauseo is still switching accounts. Try again in a moment.");
  expect(field.value).toBe("sk-ant-api03-kept");
  expect(t.setups()).toHaveLength(1);
});

test("agent.key_unreadable shows as failed, in words-errors' words, with Sign in again and the key panel", async () => {
  const t = await at({ failed: { code: "agent.key_unreadable", at: AT } });
  expect(block("claude")!.dataset["state"]).toBe("failed");
  expect(words()).toContain("Clauseo couldn't check for a saved API key in time.");
  expect(block("claude")!.querySelector('[data-testid="plans-key"]')).not.toBeNull();
  await press(button("Sign in again", block("claude")!));
  expect(t.setups()).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("keys: Sign out's confirm starts on Cancel, and Escape sends nothing and gives focus back to Sign out", async () => {
  const t = await at(signed());
  const opener = document.querySelector<HTMLElement>('[data-testid="plans-sign-out"]');
  opener?.focus();
  await press(opener);
  expect(document.activeElement).toBe(
    document.querySelector('[data-testid="plans-sign-out-cancel"]'),
  );
  await act(async () => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
  });
  await settle();
  await act(async () => new Promise((resolve) => setTimeout(resolve, 80)));
  expect(document.body.textContent).not.toContain("Sign out of Claude?");
  expect(t.setups()).toEqual([]);
  expect(document.activeElement).toBe(opener);
});

test("keys: Enter in the firm's address connects, once", async () => {
  const t = await at({ signed_in: { method: "firm", source: "own", at: AT } });
  const field = block("claude")!.querySelector<HTMLInputElement>(
    '[data-testid="plans-endpoint"] input',
  )!;
  await typeInto(field, "https://ai.mehrachambers.in");
  await act(async () => field.form?.requestSubmit());
  await settle();
  expect(t.setups()).toEqual([
    { set_firm_endpoint: { agent: CLAUDE, url: "https://ai.mehrachambers.in" } },
  ]);
});
