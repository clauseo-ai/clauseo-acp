// The keychain row's pointer across the main window (PR #1350's review): the rows read again when the window comes back
// to the front, once per window, never stacked; one rule (permission-rows.ts's awaitsAllow) for the band and every
// pointer, not_asked and the keychain's unknown included; macOS asking shared by every screen, ended by whatever a later
// read says (a crashed ask's denied too); the band's Not now on a refused keychain, this window's only; Windows with no
// keychain row points nowhere and says its own line; focus lands on what takes the Allow's place after a grant.
// @vitest-environment happy-dom
import { act } from "react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import {
  heardOf,
  providePermissions,
  type PermissionRow,
  type PermissionsHost,
} from "./permissions-host.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
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
  settle,
  signIn,
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

afterEach(() => vi.restoreAllMocks());

const KEYS: PermissionRow = {
  id: "keychain",
  state: "would_ask",
  folder: null,
  settings_path: null,
  note: null,
};
const POINTER = "Clauseo needs your Mac's permission to read its saved keys.";
const ASKING = "macOS is asking. Type your Mac's login password, then choose Always Allow.";
const KEY_WORDS = "Clauseo couldn't check for a saved API key in time.";
const UNREADABLE: SignIn = { failed: { code: "agent.key_unreadable", at: AT } };
const first = [info(true), install(CLAUDE, READY), signIn(CLAUDE, UNREADABLE)];

/** A host whose rows the test changes (`now`), counting its reads; asks wait for `answer` unless it is set. */
function desktop(rows: PermissionRow[]) {
  const d = {
    now: rows,
    reads: 0,
    held: null as null | ((rows: PermissionRow[]) => void),
    answer: undefined as undefined | ((row: PermissionRow) => void),
    host: null as unknown as PermissionsHost & { ask: ReturnType<typeof vi.fn> },
  };
  d.host = {
    list: () => {
      d.reads++;
      if (d.held !== null) return new Promise((resolve) => (d.held = resolve));
      return Promise.resolve(d.now);
    },
    ask: vi.fn(
      () =>
        new Promise<PermissionRow>((resolve) => {
          d.answer = resolve;
        }),
    ),
  };
  return d;
}

const gate = () => byId("signin-state");
const gateAllow = () => gate()?.querySelector<HTMLButtonElement>('[data-testid="keychain-allow"]');

async function focusWindow() {
  await act(async () => void window.dispatchEvent(new Event("focus")));
  await settle();
}

test("a grant made elsewhere, read as the window gains focus: the band leaves and the gate says today's words", async () => {
  const d = desktop([KEYS]);
  await mountFirstRun({ first, permissions: d.host });
  expect(byId("permissions-band")).not.toBeNull();
  expect(gate()?.textContent).toContain(POINTER);
  d.now = [{ ...KEYS, state: "granted" }];
  await focusWindow();
  expect(byId("permissions-band")).toBeNull();
  expect(gate()?.textContent).toContain(KEY_WORDS);
  expect(gateAllow()).toBeNull();
});

test("visibilitychange to visible reads again; to hidden it doesn't", async () => {
  const d = desktop([KEYS]);
  await mountFirstRun({ first, permissions: d.host });
  const before = d.reads;
  const seen = (state: DocumentVisibilityState) =>
    act(async () => {
      Object.defineProperty(document, "visibilityState", { value: state, configurable: true });
      document.dispatchEvent(new Event("visibilitychange"));
    });
  d.now = [{ ...KEYS, state: "granted" }];
  await seen("hidden");
  await settle();
  expect(d.reads).toBe(before);
  await seen("visible");
  await settle();
  expect(d.reads).toBe(before + 1);
  expect(gate()?.textContent).toContain(KEY_WORDS);
});

test("one focus listener per window however many screens listen, one read in flight at a time, and none left after", async () => {
  const added = vi.spyOn(window, "addEventListener");
  const removed = vi.spyOn(window, "removeEventListener");
  const d = desktop([KEYS]);
  const transport = {} as HostTransport;
  providePermissions(transport, d.host);
  const h = heardOf(transport);
  const screens = [h.subscribe(() => {}), h.subscribe(() => {}), h.subscribe(() => {})];
  expect(added.mock.calls.filter(([type]) => type === "focus")).toHaveLength(1);
  d.held = () => {};
  window.dispatchEvent(new Event("focus"));
  window.dispatchEvent(new Event("focus"));
  window.dispatchEvent(new Event("focus"));
  expect(d.reads).toBe(1);
  for (const leave of screens) leave();
  expect(removed.mock.calls.filter(([type]) => type === "focus")).toHaveLength(1);
  window.dispatchEvent(new Event("focus"));
  expect(d.reads).toBe(1);
});

test.each([
  ["denied", "denied"],
  ["not_asked", "not_asked"],
  ["unknown", "unknown"],
] as const)(
  "keychain %s: the gate and the first screen point at Allow, as the band does",
  async (_, state) => {
    await mountFirstRun({ first, permissions: desktop([{ ...KEYS, state }]).host });
    expect(byId("permissions-band")).not.toBeNull();
    expect(gate()?.textContent).toContain(POINTER);
    expect(gateAllow()?.disabled).toBe(false);
  },
);

test.each([["denied"], ["not_asked"], ["unknown"]] as const)(
  "keychain %s on the first screen: its plan row points at Allow",
  async (state) => {
    await mountFirstRun({ matters: [], first, permissions: desktop([{ ...KEYS, state }]).host });
    expect(byId("signin-foot")?.dataset["state"]).toBe("keychain");
    expect(byId("keychain-allow")?.textContent).toBe("Allow");
  },
);

test("keychain asking (the desktop's own state): the gate waits with Allow disabled, as the band does", async () => {
  await mountFirstRun({ first, permissions: desktop([{ ...KEYS, state: "asking" }]).host });
  expect(byId("permission-waiting")?.textContent).toBe(ASKING);
  expect(gate()?.textContent).toContain(ASKING);
  expect(gateAllow()?.disabled).toBe(true);
});

test("an ask from the gate: every screen in the window waits, with one ask; granted, focus lands on the gate's line", async () => {
  const d = desktop([KEYS]);
  await mountFirstRun({ first, permissions: d.host });
  await press(gateAllow() ?? null);
  expect(byId("permission-waiting")?.textContent).toBe(ASKING);
  expect(byId("permission-allow")).toBeNull();
  expect(gate()?.textContent).toContain(ASKING);
  expect(gateAllow()?.disabled).toBe(true);
  await press(gateAllow() ?? null);
  expect(d.host.ask).toHaveBeenCalledTimes(1);
  d.now = [{ ...KEYS, state: "granted" }];
  await act(async () => d.answer?.({ ...KEYS, state: "granted" }));
  await settle();
  expect(gate()?.textContent).toContain(KEY_WORDS);
  await vi.waitFor(() => {
    const focused = document.activeElement as HTMLElement | null;
    expect(gate()?.contains(focused)).toBe(true);
    expect(focused?.textContent).toContain(KEY_WORDS);
  });
});

test("asking, then a read of denied (a crashed or timed-out ask): denied with its Allow, nothing waiting", async () => {
  const d = desktop([KEYS]);
  await mountFirstRun({ first, permissions: d.host });
  await press(byId("permission-allow"));
  expect(byId("permission-waiting")).not.toBeNull();
  d.now = [{ ...KEYS, state: "denied" }];
  await focusWindow();
  expect(byId("permission-waiting")).toBeNull();
  expect(byId("permission-denied")).not.toBeNull();
  expect(byId("permission-allow")).not.toBeNull();
  expect(gate()?.textContent).toContain(POINTER);
  expect(gate()?.textContent).not.toContain(ASKING);
  expect(gateAllow()?.disabled).toBe(false);
});

test("an ask that never answers, then a read of would_ask: Allow asks again, and the old ask's late answer is stale", async () => {
  const d = desktop([KEYS]);
  await mountFirstRun({ first, permissions: d.host });
  await press(byId("permission-allow"));
  expect(d.host.ask).toHaveBeenCalledTimes(1);
  expect(byId("permission-waiting")).not.toBeNull();
  const late = d.answer!;
  await focusWindow();
  expect(byId("permission-waiting")).toBeNull();
  await press(byId("permission-allow"));
  expect(d.host.ask).toHaveBeenCalledTimes(2);
  expect(byId("permission-waiting")).not.toBeNull();
  // the first ask's answer, late: it settles harmlessly, the second ask still waits
  await act(async () => late({ ...KEYS, state: "granted" }));
  await settle();
  expect(byId("permission-waiting")).not.toBeNull();
  expect(byId("permissions-band")).not.toBeNull();
  expect(gate()?.textContent).toContain(ASKING);
  // the second ask's answer is the one heard
  await act(async () => d.answer!({ ...KEYS, state: "denied" }));
  await settle();
  expect(byId("permission-waiting")).toBeNull();
  expect(byId("permission-denied")).not.toBeNull();
});

test("Not now on a refused keychain puts the band away, stored nowhere; the gate still points at Allow", async () => {
  await mountFirstRun({ first, permissions: desktop([{ ...KEYS, state: "denied" }]).host });
  expect(byId("permission-not-now")?.textContent).toBe("Not now");
  await press(byId("permission-not-now"));
  expect(byId("permissions-band")).toBeNull();
  expect(gate()?.textContent).toContain(POINTER);
  expect(localStorage.length).toBe(0);
  expect(sessionStorage.length).toBe(0);
});

test("no Not now on a keychain row that only would ask", async () => {
  await mountFirstRun({ first, permissions: desktop([KEYS]).host });
  expect(byId("permissions-band")).not.toBeNull();
  expect(byId("permission-not-now")).toBeNull();
});

test("Windows, no keychain row: no pointer anywhere, and agent.key_denied says Windows' line, no Mac sentence", async () => {
  const denied: SignIn = { failed: { code: "agent.key_denied", at: AT } };
  await mountFirstRun({
    platform: "windows",
    first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, denied)],
    permissions: desktop([]).host,
  });
  expect(byId("keychain-allow")).toBeNull();
  expect(document.body.textContent).not.toContain(POINTER);
  expect(gate()?.textContent).toContain(
    "Clauseo couldn't read, save or remove the API key on this computer. Try again.",
  );
  expect(document.body.textContent).not.toContain("Mac");
});
