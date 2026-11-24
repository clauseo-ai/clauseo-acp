// The permissions band for a lawyer who already has matters (issue 5, #1303): with a row not asked yet, the step stands
// over the room, headed "Before you go on", and the room stays usable; with every row asked or none, no band.
// @vitest-environment happy-dom
import { expect, test, vi } from "vitest";
import type { PermissionRow, PermissionsHost } from "./first-run/permissions-host.ts";
import { A, byId, mountFirstRun, press } from "./first-run/rig.tsx";

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

test("a lawyer with matters and a row not asked yet sees the band over the room, which stays usable", async () => {
  const fake = host([KEYS]);
  await mountFirstRun({ matters: [A], permissions: fake });
  expect(byId("first-screen")).toBeNull();
  const band = byId("permissions-band");
  expect(band?.querySelector("h2")?.textContent).toBe("Before you go on");
  expect(band?.textContent).toContain("Your keychain");
  // the matter is open under it
  expect(byId("matter")?.textContent).toContain(A.name);
  await press(band?.querySelector<HTMLElement>('[data-testid="permission-allow"]') ?? null);
  expect(fake.ask).toHaveBeenCalledTimes(1);
  expect(byId("permission-granted")?.textContent).toBe("Allowed");
});

test("no band when every row was asked already, or there is none", async () => {
  await mountFirstRun({ matters: [A], permissions: host([{ ...KEYS, state: "denied" }]) });
  expect(byId("matter")).not.toBeNull();
  expect(byId("permissions-band")).toBeNull();
});

test("no band with no rows", async () => {
  await mountFirstRun({ matters: [A], permissions: host([]) });
  expect(byId("matter")).not.toBeNull();
  expect(byId("permissions-band")).toBeNull();
});
