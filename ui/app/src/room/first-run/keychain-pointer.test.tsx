// The keychain row's pointer for an unreadable agent key (stage E, Mac keychain prompting off): while the window's one
// permissions read lists the keychain row as would_ask, the composer's gate and the first screen's plan row say
// Clauseo needs the Mac's permission and offer the row's Allow, pressed through to the same ask, never Try again or a
// sign-in; with the row granted, or not listed, they keep today's words.
// @vitest-environment happy-dom
import { beforeAll, expect, test, vi } from "vitest";
import type { PermissionRow, PermissionsHost } from "./permissions-host.ts";
import { KEYCHAIN_CODES } from "./keychain-state.ts";
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
  signIn,
  text,
} from "./rig.tsx";

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

const KEYS: PermissionRow = {
  id: "keychain",
  state: "would_ask",
  folder: null,
  settings_path: null,
  note: null,
};
const host = (rows: PermissionRow[]): PermissionsHost & { ask: ReturnType<typeof vi.fn> } => ({
  list: async () => rows,
  ask: vi.fn(async () => ({ ...KEYS, state: "granted" as const })),
});
const UNREADABLE: SignIn = { failed: { code: "agent.key_unreadable", at: AT } };
const POINTER = "Clauseo needs your Mac's permission to read its saved keys.";
const PLANS = "To change it, open Settings, then Plans.";
const KEY_WORDS = "Clauseo couldn't check for a saved API key in time.";
const first = [info(true), install(CLAUDE, READY), signIn(CLAUDE, UNREADABLE)];

test("the composer's gate, keychain would_ask: the pointer and the row's Allow, never Try again or the Plans line", async () => {
  const fake = host([KEYS]);
  await mountFirstRun({ first, permissions: fake });
  const gate = byId("signin-state");
  expect(gate?.textContent).toContain(POINTER);
  expect(gate?.textContent).not.toContain(PLANS);
  expect(gate?.textContent).not.toContain("Try again");
  expect(gate?.textContent).not.toContain("Sign in");
  // the gate's Allow asks for the keychain row, the same ask as the band's; granted, the gate says today's words
  await press(gate?.querySelector<HTMLElement>('[data-testid="keychain-allow"]') ?? null);
  expect(fake.ask).toHaveBeenCalledWith("keychain");
  expect(byId("signin-state")?.textContent).toContain(KEY_WORDS);
  expect(byId("permissions-band")).toBeNull();
});

test("the first screen's plan row, keychain would_ask: the pointer and Allow, never Try again or a sign-in", async () => {
  await mountFirstRun({ matters: [], first, permissions: host([KEYS]) });
  const row = document.querySelector(`[data-testid="plan-row"][data-agent="${CLAUDE}"]`);
  expect(byId("signin-foot")?.dataset["state"]).toBe("keychain");
  expect(row?.textContent).toContain(POINTER);
  expect(row?.textContent).not.toContain(PLANS);
  expect(byId("signin-foot-sign-in")).toBeNull();
  expect(byId("keychain-allow")?.textContent).toBe("Allow");
  expect(text()).not.toContain("Sign in with Claude");
});

const KEPT = [
  ["granted", [{ ...KEYS, state: "granted" as const }]],
  ["not listed", []],
] as const;

test.each(KEPT)("keychain %s: the gate keeps today's words", async (_, rows) => {
  await mountFirstRun({ first, permissions: host([...rows]) });
  expect(byId("signin-state")?.textContent).toContain(PLANS);
  expect(byId("signin-state")?.textContent).toContain("Try again");
  expect(text()).not.toContain(POINTER);
});

test.each(KEPT)("keychain %s: the first screen keeps today's words", async (_, rows) => {
  await mountFirstRun({ matters: [], first, permissions: host([...rows]) });
  expect(byId("signin-foot")?.dataset["state"]).toBe("key-unreadable");
  expect(byId("signin-foot-sign-in")?.textContent).toBe("Try again");
  expect(text()).not.toContain(POINTER);
});

test("a keychain write failure in KEYCHAIN_CODES (a stand-in code here) points at the keychain row too", async () => {
  const STAND_IN = "stand_in.keychain_write";
  KEYCHAIN_CODES.push(STAND_IN);
  try {
    const failed: SignIn = { failed: { code: STAND_IN as never, at: AT } };
    await mountFirstRun({
      first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, failed)],
      permissions: host([KEYS]),
    });
    expect(byId("signin-state")?.textContent).toContain(POINTER);
    expect(byId("signin-state")?.querySelector('[data-testid="keychain-allow"]')).not.toBeNull();
    expect(byId("signin-state")?.textContent).not.toContain("Sign-in didn't finish.");
  } finally {
    KEYCHAIN_CODES.splice(KEYCHAIN_CODES.indexOf(STAND_IN), 1);
  }
});

test("the gate's Allow answered with a plain Allow (still would_ask): it says allowed once only, Allow again", async () => {
  const fake: PermissionsHost = { list: async () => [KEYS], ask: async () => KEYS };
  await mountFirstRun({ first, permissions: fake });
  await press(
    byId("signin-state")?.querySelector<HTMLElement>('[data-testid="keychain-allow"]') ?? null,
  );
  const once = "macOS allowed it once only. Press Allow again and choose Always Allow.";
  expect(byId("signin-state")?.textContent).toContain(once);
  expect(byId("permission-once")?.textContent).toBe(once);
  expect(byId("permission-allow")).not.toBeNull();
});
