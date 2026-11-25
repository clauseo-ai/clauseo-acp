// @vitest-environment happy-dom
// The keychain row's pointer in the settings window (PR #1350's review): Plans and Connectors' Case Law follow the one
// rule (permission-rows.ts's awaitsAllow), denied, not_asked and unknown included; they wait while macOS asks; a grant
// made in another window is read as this one gains focus; Windows with no keychain row points nowhere and says its own
// agent.key_denied line.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import type { PermissionRow, PermissionsHost } from "../../room/first-run/permissions-host.ts";
import type { SignIn } from "../../transport/protocol.ts";
import { mountSettings, stateOf, type Rig } from "./connectors-rig.tsx";
import {
  AT,
  CLAUDE,
  READY,
  block,
  install,
  mountPlans,
  settle,
  signIn,
  unmount,
} from "./plans-rig.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

let rig: Rig | null = null;
beforeEach(() => localStorage.clear());
afterEach(async () => {
  await rig?.unmount();
  rig = null;
  await unmount();
});

const KEYS: PermissionRow = {
  id: "keychain",
  state: "would_ask",
  folder: null,
  settings_path: null,
  note: null,
};
const POINTER = "Clauseo needs your Mac's permission to read its saved keys.";
const ASKING = "macOS is asking. Type your Mac's login password, then choose Always Allow.";
const TIMED_OUT = "Clauseo couldn't check for a saved API key in time.";
const failed = (code: string): SignIn => ({ failed: { code: code as never, at: AT } });

/** A host whose rows the test changes (`now`); asks never answer. */
function desktop(rows: PermissionRow[]) {
  const d = { now: rows, host: null as unknown as PermissionsHost };
  d.host = { list: async () => d.now, ask: () => new Promise<PermissionRow>(() => {}) };
  return d;
}

const plans = (d: { host: PermissionsHost }, code = "agent.key_unreadable", platform?: "windows") =>
  mountPlans({
    first: [install(CLAUDE, READY), signIn(CLAUDE, failed(code))],
    permissions: d.host,
    ...(platform === undefined ? {} : { platform }),
  });
const claude = () => block("claude")?.textContent ?? "";
const plansAllow = () =>
  block("claude")?.querySelector<HTMLButtonElement>('[data-testid="keychain-allow"]') ?? null;

async function focusWindow() {
  await act(async () => void window.dispatchEvent(new Event("focus")));
  await settle();
}

test.each([["would_ask"], ["denied"], ["not_asked"], ["unknown"]] as const)(
  "Plans, keychain %s: the pointer and Allow, never Try again",
  async (state) => {
    await plans(desktop([{ ...KEYS, state }]));
    expect(claude()).toContain(POINTER);
    expect(plansAllow()?.disabled).toBe(false);
    expect(block("claude")?.querySelector('[data-testid="plans-sign-in"]')).toBeNull();
  },
);

test("Plans, keychain asking: it waits, Allow disabled", async () => {
  await plans(desktop([{ ...KEYS, state: "asking" }]));
  expect(claude()).toContain(ASKING);
  expect(plansAllow()?.disabled).toBe(true);
});

test("Plans: a grant read as the settings window gains focus puts back today's words and Try again", async () => {
  const d = desktop([KEYS]);
  await plans(d);
  expect(claude()).toContain(POINTER);
  d.now = [{ ...KEYS, state: "granted" }];
  await focusWindow();
  expect(claude()).toContain(TIMED_OUT);
  expect(plansAllow()).toBeNull();
  expect(block("claude")?.querySelector('[data-testid="plans-sign-in"]')?.textContent).toBe(
    "Try again",
  );
});

test("Plans on Windows, no keychain row: no pointer, and agent.key_denied says Windows' line with no Mac sentence", async () => {
  await plans(desktop([]), "agent.key_denied", "windows");
  expect(plansAllow()).toBeNull();
  expect(claude()).toContain(
    "Clauseo couldn't read, save or remove the API key on this computer. Try again. If it keeps happening, show this to whoever looks after this computer.",
  );
  expect(claude()).not.toContain("Mac");
});

const caseLaw = (d: { host: PermissionsHost }) =>
  mountSettings({
    state: stateOf({ status: "signed_out" }),
    hash: "#connectors",
    permissions: d.host,
  });

test.each([["would_ask"], ["denied"], ["not_asked"], ["unknown"]] as const)(
  "Case Law signed out, keychain %s: the pointer and Allow, never Sign in",
  async (state) => {
    rig = await caseLaw(desktop([{ ...KEYS, state }]));
    expect(rig.text()).toContain(POINTER);
    expect(rig.buttons()).toEqual(["Allow", "Remove", "Turn off"]);
  },
);

test("Case Law, keychain asking: it waits, Allow disabled", async () => {
  rig = await caseLaw(desktop([{ ...KEYS, state: "asking" }]));
  expect(rig.text()).toContain(ASKING);
  expect(rig.el.querySelector<HTMLButtonElement>('[data-testid="keychain-allow"]')?.disabled).toBe(
    true,
  );
});

test("Case Law: a grant read as the settings window gains focus puts back Sign in", async () => {
  const d = desktop([KEYS]);
  rig = await caseLaw(d);
  d.now = [{ ...KEYS, state: "granted" }];
  await focusWindow();
  expect(rig.text()).toContain("Signed out. Sign in to search case law again.");
  expect(rig.buttons()).toEqual(["Sign in", "Use an API key instead", "Remove", "Turn off"]);
});

test("Case Law on Windows' rows (no keychain row): no pointer", async () => {
  rig = await caseLaw(desktop([]));
  expect(rig.text()).not.toContain(POINTER);
  expect(rig.buttons()).toEqual(["Sign in", "Use an API key instead", "Remove", "Turn off"]);
});
