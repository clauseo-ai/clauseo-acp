// The permissions band for a lawyer who already has matters (issue 5, #1303): with a row not asked yet, the step stands
// over the room, headed "Before you go on", and the room stays usable; it leaves once every row is allowed, and stays
// while a row was refused; with every row asked already, or none, no band.
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
const DOCS: PermissionRow = {
  ...KEYS,
  id: "files_documents",
  state: "not_asked",
  folder: "Documents",
};
/** A host listing `rows`, each ask answered with its row allowed. */
const host = (rows: PermissionRow[]): PermissionsHost & { ask: ReturnType<typeof vi.fn> } => ({
  list: async () => rows,
  ask: vi.fn(async (id: PermissionRow["id"]) => ({
    ...rows.find((r) => r.id === id)!,
    state: "granted" as const,
  })),
});

test("a lawyer with matters and a row not asked yet sees the band over the room, which stays usable; it leaves once every row is allowed", async () => {
  const fake = host([DOCS, KEYS]);
  await mountFirstRun({ matters: [A], permissions: fake });
  expect(byId("first-screen")).toBeNull();
  const band = () => byId("permissions-band");
  expect(band()?.querySelector("h2")?.textContent).toBe("Before you go on");
  expect(band()?.textContent).toContain("Your keychain");
  // the matter is open under it
  expect(byId("matter")?.textContent).toContain(A.name);
  // Documents allowed: the keychain still waits its turn, so the band stays
  await press(band()?.querySelector<HTMLElement>('[data-testid="permission-allow"]') ?? null);
  expect(band()?.querySelector('[data-testid="permission-granted"]')?.textContent).toBe("Allowed");
  // the keychain allowed too: every row allowed, the band leaves
  await press(band()?.querySelector<HTMLElement>('[data-testid="permission-allow"]') ?? null);
  expect(fake.ask).toHaveBeenCalledTimes(2);
  expect(band()).toBeNull();
  expect(byId("matter")).not.toBeNull();
});

test("a row refused in this session keeps the band, with its Allow", async () => {
  const fake: PermissionsHost = {
    list: async () => [KEYS],
    ask: async () => ({ ...KEYS, state: "denied" as const }),
  };
  await mountFirstRun({ matters: [A], permissions: fake });
  await press(byId("permission-allow"));
  expect(byId("permissions-band")).not.toBeNull();
  expect(byId("permission-denied")).not.toBeNull();
  expect(byId("permission-allow")).not.toBeNull();
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
