// The scan of a folder picked in New matter (UD2, room/scan/), over FakeTransport: one scan_folder per pick, the counts
// and the file reached from folder_scanned for that folder alone, Stop, the synced notice Create waits for, the
// cloud-only lines, Create during a scan, a refused scan, and the services' names.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createErrorLine } from "@clauseo/components/clauseo/matter-form-check.ts";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import type {
  CommandEnvelope,
  CommandOutcome,
  FolderScanned,
  HostQuery,
  MatterSummary,
  SyncService,
} from "../../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../../transport/fake-transport.ts";
import { uuid } from "../../testing/stream-script.ts";
import { Room } from "../room.tsx";
import { connectRoom } from "../start.ts";
import { serviceOf } from "./use-scan.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const MADE = uuid(5, 2);
const NAME = "Mehta v. Mehta";
const REF = "ref-a";

const scanned = (over: Partial<FolderScanned> = {}): FolderScanned => ({
  folder: REF,
  files: 24,
  bytes: 38_200_000,
  place: "local",
  cloud_only: 0,
  done: false,
  cancelled: false,
  ...over,
});

/** A host with the app feed, picking REF, its scan playing `events`; scan_folder refused with `refuse`. */
function scanning(events: FolderScanned[], refuse?: string): FakeHost {
  let list: MatterSummary[] = [A];
  return {
    query: (query: HostQuery) =>
      typeof query === "object" && "folder_support" in query
        ? { folder_support: {} }
        : { matters: list },
    dispatch: (command: CommandEnvelope): CommandOutcome => {
      if (refuse !== undefined && "scan_folder" in command.command)
        throw { code: refuse, message: "refused", retryable: false };
      if (!("create_matter" in command.command)) return "done";
      list = [A, { matter: MADE, name: command.command.create_matter.name, kind: "folder" }];
      return { matter_created: { matter: MADE } };
    },
    pickFolder: () => ({ folder: REF, display: "/Users/rm/Matters/Rathi Steels" }),
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    scan: () => events,
  };
}

async function settle() {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

async function mount(h: FakeHost) {
  const transport = new FakeTransport(h);
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  return transport;
}

const byId = <E extends HTMLElement>(id: string) =>
  document.querySelector<E>(`[data-testid="${id}"]`);

async function press(id: string) {
  await act(async () => byId(id)?.click());
  await settle();
}

/** New matter, its name typed, and Change: REF picked. */
async function pick(transport: FakeTransport) {
  await act(async () => byId("new-matter")?.click());
  await act(async () => settled());
  const input = byId<HTMLInputElement>("matter-name")!;
  await act(async () => {
    Reflect.set(HTMLInputElement.prototype, "value", NAME, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await press("matter-existing");
  return transport;
}

const commands = (transport: FakeTransport) =>
  transport.only("dispatch").map((d) => d.command.command);
const named = (transport: FakeTransport, name: string) =>
  commands(transport).filter((c) => name in c);
const play = async (transport: FakeTransport, event: FolderScanned) => {
  await act(async () => transport.appEvent({ folder_scanned: event }));
  await settle();
};
const text = (id: string) => byId(id)?.textContent ?? null;

test("a pick sends one scan_folder with the picked ref; the counts and the file reached show, then done drops the reading line", async () => {
  const transport = await pick(
    await mount(scanning([scanned({ now_reading: "Board/Board minutes.docx" })])),
  );
  expect(named(transport, "scan_folder")).toEqual([{ scan_folder: { folder: REF } }]);
  expect(text("scan")).toContain("24 files, 38 MB");
  expect(text("scan")).toContain("Reading the folder. Now \u2068Board minutes.docx\u2069.");
  expect(byId("scan")?.hasAttribute("data-done")).toBe(false);
  await play(transport, scanned({ files: 1, bytes: 400_000 }));
  expect(text("scan")).toContain("1 file, 1 MB");
  expect(text("scan")).toContain("Reading the folder.");
  // another folder's event changes nothing
  await play(transport, scanned({ folder: "ref-other", files: 99 }));
  expect(text("scan")).toContain("1 file, 1 MB");
  await play(transport, scanned({ files: 30, bytes: 52_000_000, done: true }));
  expect(text("scan")).toContain("30 files, 52 MB");
  expect(text("scan")).not.toContain("Reading the folder");
  expect(byId("scan")?.hasAttribute("data-done")).toBe(true);
  expect(byId("scan-stop")).toBeNull();
});

test("an empty folder reads 0 files, 0 MB", async () => {
  await pick(await mount(scanning([scanned({ files: 0, bytes: 0, done: true })])));
  expect(text("scan")).toContain("0 files, 0 MB");
});

test("Stop sends cancel_scan and drops the pick: the lines go, the folder line is as before, and no create takes the folder", async () => {
  const transport = await pick(await mount(scanning([scanned()])));
  expect(document.body.textContent).toContain("Rathi Steels");
  await press("scan-stop");
  expect(named(transport, "cancel_scan")).toEqual([{ cancel_scan: { folder: REF } }]);
  expect(byId("scan")).toBeNull();
  expect(document.body.textContent).not.toContain("Rathi Steels");
  await play(transport, scanned({ done: true, cancelled: true }));
  expect(byId("scan")).toBeNull();
  expect(
    named(transport, "create_matter").filter((c) => JSON.stringify(c).includes("existing_folder")),
  ).toEqual([]);
});

test("a synced folder: the notice, and Create waits for Got it, once per adding", async () => {
  const transport = await pick(
    await mount(scanning([scanned({ place: { synced: { service: "one_drive" } }, done: true })])),
  );
  expect(byId("place-notice")?.dataset["place"]).toBe("synced");
  expect(text("place-notice")).toContain(
    "This folder syncs with OneDrive, so changes to its papers sync too, including Clauseo's. Versions are kept on this computer, outside the folder.",
  );
  await press("create-matter");
  expect(named(transport, "create_matter")).toEqual([]);
  await press("synced-got-it");
  expect(byId("place-notice")).toBeNull();
  await press("create-matter");
  expect(named(transport, "create_matter")).toEqual([
    { create_matter: { name: NAME, source: { existing_folder: { folder: REF } } } },
  ]);
});

test("cloud-only files, by the service's name where synced and online where not", async () => {
  await pick(
    await mount(
      scanning([
        scanned({ place: { synced: { service: "one_drive" } }, cloud_only: 6, done: true }),
      ]),
    ),
  );
  expect(text("scan-cloud-only")).toBe(
    "6 files are kept only in OneDrive and download as they're read.",
  );
  await act(async () => root?.unmount());
  host?.remove();
  await pick(await mount(scanning([scanned({ cloud_only: 1, done: true })])));
  expect(text("scan-cloud-only")).toBe("1 file is kept only online and downloads as it's read.");
});

test("a network folder adds no line of UD2's own", async () => {
  await pick(await mount(scanning([scanned({ place: "network", done: true })])));
  expect(byId("place-notice")).toBeNull();
  expect(byId("scan-cloud-only")).toBeNull();
});

test("Create while the scan runs sends cancel_scan, then create_matter", async () => {
  const transport = await pick(await mount(scanning([scanned()])));
  await press("create-matter");
  const order = commands(transport)
    .map((c) => Object.keys(c)[0])
    .filter((k) => k === "cancel_scan" || k === "create_matter");
  expect(order).toEqual(["cancel_scan", "create_matter"]);
});

test("a refused scan says its code's line, as Create would, and holds Create back", async () => {
  const transport = await pick(await mount(scanning([], "matter.folder_is_drive")));
  expect(document.body.textContent).toContain(
    createErrorLine("en", "matter.folder_is_drive", "/Users/rm/Matters/Rathi Steels"),
  );
  await press("create-matter");
  expect(named(transport, "create_matter")).toEqual([]);
});

test("the services by their owners' names; other, and one this version doesn't know, have none", () => {
  const of = (service: string) =>
    serviceOf(scanned({ place: { synced: { service: service as SyncService } } }));
  expect(of("one_drive")).toBe("OneDrive");
  expect(of("google_drive")).toBe("Google Drive");
  expect(of("icloud")).toBe("iCloud Drive");
  expect(of("dropbox")).toBe("Dropbox");
  expect(of("box")).toBe("Box");
  expect(of("other")).toBeNull();
  expect(of("a_service_from_later")).toBeNull();
  expect(serviceOf(scanned({ place: "local" }))).toBeNull();
});
