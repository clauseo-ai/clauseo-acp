// A refused Locate folder (folder refusals): the open matter whose folder moved says why under the moved folder's words,
// naming the folder picked, with Locate folder still there to pick another; the next Locate clears it, and a Locate
// that takes clears it for good. A matter that isn't open says it on its rail row's note line, with Locate folder
// beside it. The store (locate-refusals.ts) drops a refusal once a folder that was away is found.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import type { MatterSummary } from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { MatterInfo, NextHostError, NextOutcome } from "./host-next.ts";
import { LocateRefusals, locateLine } from "./locate-refusals.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const KHAN: MatterSummary = { matter: uuid(1, 2), name: "Khan Leather Works", kind: "folder" };
const ACME: MatterSummary = { matter: uuid(2, 2), name: "Acme Ltd v. State", kind: "folder" };
const iso = (text: string) => `\u2068${text}\u2069`;
const PICKED = iso("~/Desktop/Khan");

const refusal = (code: string, matter?: string): NextHostError =>
  ({
    code,
    message: "SECRET-DETAIL",
    retryable: false,
    ...(matter === undefined ? {} : { matter }),
  }) as NextHostError;

/** A NextHost listing `list()`, whose relocate_matter answers `answers` in turn: a refusal is thrown, null takes. */
function locating(list: () => MatterInfo[], answers: (NextHostError | null)[]): FakeHost {
  return {
    pickFolder: () => ({ folder: "ref-found", display: "Desktop/Khan" }),
    next: fakeNext({
      matters: list,
      command: (c): NextOutcome | undefined => {
        if (!("relocate_matter" in c)) return undefined;
        const answer = answers.shift() ?? null;
        if (answer !== null) throw answer;
        return undefined;
      },
    }),
  };
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

async function settle() {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

const byId = (id: string) => host?.querySelector<HTMLElement>(`[data-testid="${id}"]`) ?? null;
const refused = () => byId("locate-refused")?.textContent ?? null;

async function locate() {
  await act(async () => byId("moved-locate")?.click());
  await settle();
}

test("the moved folder says why a Locate was refused, naming the folder; the next Locate clears it", async () => {
  let found = false;
  const list = () => [
    detailed(KHAN, {
      folder_state: found ? "found" : "missing",
      folder_display: "Documents/Khan",
      last_opened: 1,
    }),
    detailed(ACME),
  ];
  const answers = [
    refusal("matter.folder_is_matter", KHAN.matter),
    refusal("matter.folder_not_a_matter"),
    refusal("matter.folder_is_matter", ACME.matter),
  ];
  const transport = await mount(locating(list, answers));
  expect(refused()).toBeNull();
  await locate();
  expect(refused()).toBe(`${PICKED} is already this matter's folder.`);
  expect(host?.textContent).not.toContain("SECRET-DETAIL");
  // under the moved folder's words, Locate folder still there to pick another
  expect(byId("locate-refused")?.closest(".room-moved")).not.toBeNull();
  expect(byId("moved-locate")).not.toBeNull();
  await locate();
  expect(refused()).toBe(
    `${PICKED} isn't this matter's folder. Choose the folder you moved it to.`,
  );
  await locate();
  expect(refused()).toBe(
    `${iso(ACME.name)} already uses ${PICKED}. Choose this matter's own folder.`,
  );
  // a Locate that takes: nothing refused, and the folder found
  found = true;
  await locate();
  expect(transport.only("pick_folder")).toHaveLength(4);
  expect(refused()).toBeNull();
  expect(byId("moved-locate")).toBeNull();
});

test("a matter that isn't open says its refused Locate on its rail row, with Locate folder beside it", async () => {
  const list = () => [
    detailed(ACME, { last_opened: 1 }),
    detailed(KHAN, { folder_state: "missing", folder_display: "Documents/Khan" }),
  ];
  await mount(locating(list, [refusal("matter.folder_missing")]));
  const row = () =>
    [...(host?.querySelectorAll<HTMLElement>('[data-testid="matter-row"]') ?? [])].find((r) =>
      r.textContent?.includes(KHAN.name),
    );
  await act(async () =>
    row()?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })),
  );
  await settle();
  const item = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((i) =>
    i.textContent?.includes("Locate folder"),
  );
  await act(async () => item?.click());
  await settle();
  expect(row()?.querySelector(".room-matter-note")?.textContent).toBe(
    `Clauseo can't find ${PICKED}. Choose the folder again.`,
  );
  expect(row()?.closest("li")?.querySelector('[data-testid="locate-folder"]')?.textContent).toBe(
    "Locate folder",
  );
  // the open matter's moved view says nothing: its folder is where it was
  expect(byId("locate-refused")).toBeNull();
});

test("the store drops a refusal taken while the folder was away once it's found, and one of a matter off the list", () => {
  const store = new LocateRefusals();
  const away = detailed(KHAN, { folder_state: "missing" });
  store.refuse(KHAN.matter, { code: "matter.folder_refused", path: "Desktop/Khan", missing: true });
  store.refuse(ACME.matter, {
    code: "matter.folder_refused",
    path: "Desktop/Acme",
    missing: false,
  });
  store.settle([away, detailed(ACME)]);
  expect([...store.view().keys()]).toEqual([KHAN.matter, ACME.matter]);
  store.settle([detailed(KHAN), detailed(ACME)]);
  expect([...store.view().keys()]).toEqual([ACME.matter]);
  store.settle([detailed(KHAN)]);
  expect(store.view().size).toBe(0);
  // with no matter the list has, Locate's generic line
  expect(
    locateLine(
      "en",
      KHAN.matter,
      { code: "matter.folder_is_matter", matter: uuid(9, 9), path: "Desktop/Khan", missing: true },
      [away],
    ),
  ).toBe(`Clauseo can't use ${PICKED} for this matter. Choose another folder.`);
});
