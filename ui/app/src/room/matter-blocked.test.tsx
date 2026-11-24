// The open matter's line where Windows' Controlled folder access protects its folder (issue 5): only for a matter whose
// folder is protected, with Show in File Explorer and Locate folder's flow; and the matters-folder choice only where
// the host can set it, its outcome in a plain line. The room over a FakeTransport with a fake permissions host.
// @vitest-environment happy-dom
import { expect, test, vi } from "vitest";
import type { MatterSummary } from "../transport/protocol.ts";
import type { FakeTransport } from "../transport/fake-transport.ts";
import {
  fakeChoiceOver,
  type PermissionRow,
  type PermissionsHost,
} from "./first-run/permissions-host.ts";
import { A, byId, mountFirstRun, press } from "./first-run/rig.tsx";

const CFA: PermissionRow = {
  id: "controlled_folder_access",
  state: "blocking",
  folder: "C:\\Users\\Asha\\Documents\\Clauseo",
  settings_path: null,
  note: null,
};
const host = (rows: PermissionRow[], over: Partial<PermissionsHost> = {}): PermissionsHost => ({
  list: async () => rows,
  ask: async (id) => rows.find((r) => r.id === id)!,
  ...over,
});
const at = (folder_display: string): MatterSummary => ({ ...A, folder_display });

test("a matter in a protected folder shows the line, with Show in File Explorer and Choose the moved folder", async () => {
  const transport = await mountFirstRun({
    matters: [at("Documents/Clauseo/Acme Ltd v. State")],
    platform: "windows",
    permissions: host([CFA]),
  });
  const line = byId("matter-blocked");
  expect(line?.textContent).toContain(
    "Windows' Controlled folder access blocks saving in this folder",
  );
  expect(line?.textContent).toContain("such as PowerShell");
  expect(line?.textContent).toContain(
    "Move this matter's folder out of Documents in File Explorer, then choose it here.",
  );
  // no matters-folder choice while the host can't set one
  expect(byId("folder-choice")).toBeNull();
  const reveal = vi.spyOn(transport, "reveal");
  await press(byId("matter-blocked-reveal"));
  expect(reveal).toHaveBeenCalledWith({ matter: A.matter });
  const pick = vi.spyOn(transport, "pickFolder").mockResolvedValue(null);
  await press(byId("matter-blocked-locate"));
  expect(pick).toHaveBeenCalledTimes(1);
});

test("a matter in a protected folder outside the matters folder (the Desktop) shows it too", async () => {
  await mountFirstRun({
    matters: [at("Desktop/Acme")],
    platform: "windows",
    permissions: host([CFA]),
  });
  expect(byId("matter-blocked")).not.toBeNull();
});

test("a matter outside the protected folders shows no line", async () => {
  await mountFirstRun({
    matters: [at("D:\\Matters\\Acme Ltd v. State")],
    platform: "windows",
    permissions: host([CFA]),
  });
  expect(byId("matter")).not.toBeNull();
  expect(byId("matter-blocked")).toBeNull();
});

test("no Controlled folder access row: no line, even in Documents", async () => {
  await mountFirstRun({
    matters: [at("Documents/Clauseo/Acme Ltd v. State")],
    platform: "windows",
    permissions: host([]),
  });
  expect(byId("matter")).not.toBeNull();
  expect(byId("matter-blocked")).toBeNull();
});

test("where the host can set a matters folder: the picker, set_default_location with the folder picked, its line", async () => {
  let transport: FakeTransport | null = null;
  transport = await mountFirstRun({
    matters: [at("Documents/Clauseo/Acme Ltd v. State")],
    platform: "windows",
    picked: { folder: "folder-ref-9", display: "D:\\Matters" },
    permissions: host([CFA], { chooseMattersFolder: () => fakeChoiceOver(transport!)() }),
  });
  await press(byId("folder-choice"));
  const set = transport
    .only("dispatch")
    .map((c) => (c.command.command as Record<string, unknown>)["set_default_location"])
    .filter((c) => c !== undefined);
  expect(set).toEqual([{ folder: "folder-ref-9" }]);
  expect(byId("folder-choice-said")?.textContent).toBe(
    "New matters now go in D:\\Matters. Matters you already have stay where they are.",
  );
});

test("a refused choice says why in plain words", async () => {
  await mountFirstRun({
    matters: [at("Documents/Clauseo/Acme Ltd v. State")],
    platform: "windows",
    permissions: host([CFA], {
      chooseMattersFolder: async () => ({ refused: "settings.locked" }),
    }),
  });
  await press(byId("folder-choice"));
  expect(byId("folder-choice-said")?.textContent).toBe(
    "Your firm sets where new matters go, so it can't be changed here.",
  );
});
