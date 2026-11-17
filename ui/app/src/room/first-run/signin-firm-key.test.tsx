// UD1, the gate for a firm endpoint with no key saved (agent.firm_key_needed, decision record 0048): its words alone,
// no button, never "Sign-in didn't finish.": signing in can't help, only saving the firm's key in Settings, Plans.
// @vitest-environment happy-dom
import { beforeAll, expect, test } from "vitest";
import type { SignIn } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  READY,
  byId,
  info,
  install,
  mountFirstRun,
  press,
  setups,
  signIn,
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

/** The room with matter A open, Claude installed, and this sign-in state. */
const at = (state: SignIn) =>
  mountFirstRun({ first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, state)] });

test("agent.firm_key_needed: its words only, with no button and never Sign-in didn't finish", async () => {
  await at({ failed: { code: "agent.firm_key_needed", at: AT } });
  const state = byId("signin-state")?.textContent ?? "";
  expect(state).toContain(
    "Claude can't start here until an API key for your firm's gateway is saved.",
  );
  expect(state).not.toContain("Sign-in didn't finish");
  expect(state).not.toContain("Try again");
  expect(byId("signin-state")?.querySelectorAll("button")).toHaveLength(0);
});

test("agent.endpoint_key_needed: names the host it points at, never the firm's gateway, with no button", async () => {
  await at({ failed: { code: "agent.endpoint_key_needed", at: AT, host: "proxy.example" } });
  const state = byId("signin-state")?.textContent ?? "";
  expect(state).toContain(
    "Claude can't start: this computer is set to send it to \u2068proxy.example\u2069,",
  );
  expect(state).not.toContain("your firm's gateway");
  expect(state).not.toContain("Sign-in didn't finish");
  expect(byId("signin-state")?.querySelectorAll("button")).toHaveLength(0);
});

test("agent.endpoint_key_needed: with the settings window, Settings › Plans is its one button, and opens no sign-in", async () => {
  const t = await mountFirstRun({
    openSettings: true,
    first: [
      info(true),
      install(CLAUDE, READY),
      signIn(CLAUDE, {
        failed: { code: "agent.endpoint_key_needed", at: AT, host: "proxy.example" },
      }),
    ],
  });
  const buttons = [...(byId("signin-state")?.querySelectorAll("button") ?? [])];
  expect(buttons.map((b) => b.textContent)).toEqual(["Settings › Plans"]);
  await press(buttons[0] ?? null);
  expect(setups(t)).toEqual([]);
});

/** The room's sign-in screen, with the settings window, for Claude's `failed`. */
const withSettings = (failed: Extract<SignIn, { failed: unknown }>["failed"]) =>
  mountFirstRun({
    openSettings: true,
    first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, { failed })],
  });

test("agent.endpoint_key_needed with no host: names the variable, and no button, even with the settings window", async () => {
  const t = await withSettings({
    code: "agent.endpoint_key_needed",
    at: AT,
    setting: "CLAUDE_CODE_API_BASE_URL",
  });
  const state = byId("signin-state")?.textContent ?? "";
  expect(state).toContain(
    "Claude can't start: a setting on this computer sends Claude to an address that isn't a web address,",
  );
  expect(state).toContain("the setting is called \u2068CLAUDE_CODE_API_BASE_URL\u2069.");
  expect(state).not.toContain("Settings › Plans");
  expect(byId("signin-state")?.querySelectorAll("button")).toHaveLength(0);
  expect(setups(t)).toEqual([]);
});

test("agent.endpoint_key_needed with neither a host nor a setting (a socket): asks for the key, Settings › Plans its button", async () => {
  const t = await withSettings({ code: "agent.endpoint_key_needed", at: AT });
  const state = byId("signin-state")?.textContent ?? "";
  expect(state).toContain("this computer is set to send it to a different address");
  expect(state).not.toContain("isn't a web address");
  const buttons = [...(byId("signin-state")?.querySelectorAll("button") ?? [])];
  expect(buttons.map((b) => b.textContent)).toEqual(["Settings › Plans"]);
  await press(buttons[0] ?? null);
  expect(setups(t)).toEqual([]);
});

test("agent.settings_unreadable: names the setting, with no button even with the settings window", async () => {
  const t = await withSettings({
    code: "agent.settings_unreadable",
    at: AT,
    setting: "ANTHROPIC_BASE_URL",
  });
  const state = byId("signin-state")?.textContent ?? "";
  expect(state).toContain(
    "Clauseo couldn't read one of this computer's settings for Claude, so it can't tell where Claude would connect.",
  );
  expect(state).toContain(
    "Whoever looks after this computer can correct it; it's \u2068ANTHROPIC_BASE_URL\u2069.",
  );
  expect(state).not.toContain("Sign-in didn't finish");
  expect(byId("signin-state")?.querySelectorAll("button")).toHaveLength(0);
  expect(setups(t)).toEqual([]);
});
