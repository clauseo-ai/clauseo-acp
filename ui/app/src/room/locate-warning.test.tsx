// @vitest-environment happy-dom
// Locate's step for a folder Claude can't start in (FR24, FR25; locate-held.ts, locate-warning.tsx): on macOS and Linux
// a folder picked to locate a matter whose name has a backslash waits for the step, Choose another folder or Locate
// anyway; on Windows, or for a plain path, Locate relocates at once.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { fr } from "@clauseo/components/lib/words-folder.ts";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import { createViewStore } from "../store/store.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { NextCommand } from "./host-next.ts";
import { LocateWarningHost } from "./locate-warning-host.tsx";
import { Matters } from "./matters.ts";
import { MovedFolder } from "./moved.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let stop: (() => void) | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  stop?.();
  root = null;
  host = null;
  stop = null;
});

const MOVED = detailed(
  { matter: uuid(1, 2), name: "Khan Leather Works v. Customs", kind: "folder" },
  { folder_state: "missing", folder_display: "Documents/Khan" },
);
const SLASHED = "Documents/Clauseo\\ Test\\ Khan";
/** The file manager the step names: Finder on the Mac, "your file manager" on Linux. */
const FILES = { mac: "Finder", linux: "your file manager" } as const;

const byId = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`);

async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await act(async () => settled());
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  }
}

/** The moved folder's line and the room's step, over a host on `platform` whose picker gives `displays` in turn. */
async function mount(platform: NonNullable<FakeHost["platform"]>, displays: string[]) {
  const sent: NextCommand[] = [];
  let picks = 0;
  const transport = new FakeTransport({
    platform,
    pickFolder: () => {
      const n = picks++;
      return { folder: `ref-${n}`, display: displays[n] ?? "Desktop/Khan" };
    },
    next: fakeNext({ matters: () => [MOVED], command: (c) => void sent.push(c) }),
  });
  const matters = new Matters(
    transport,
    createViewStore(transport, { batch: () => () => () => {} }),
  );
  stop = matters.start();
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <>
        <MovedFolder onLocate={() => void matters.locate(MOVED.matter)} />
        <LocateWarningHost held={matters.located} />
      </>,
    ),
  );
  await settle();
  const locate = async () => {
    await act(async () => host?.querySelector("button")?.click());
    await settle();
  };
  const relocated = () =>
    sent.filter((c) => "relocate_matter" in c).map((c) => c.relocate_matter.folder);
  return { transport, locate, relocated };
}

async function press(id: string) {
  await act(async () => byId(id)?.click());
  await settle();
}

test.each(["mac", "linux"] as const)(
  "on %s, a folder whose name has a backslash waits for the step; Choose another folder picks again",
  async (platform) => {
    const { transport, locate, relocated } = await mount(platform, [SLASHED, "Desktop/Khan"]);
    await locate();
    const step = byId("locate-warning");
    expect(step).not.toBeNull();
    expect(step?.textContent).toContain("This folder's name has a backslash (\\).");
    expect(step?.textContent).toContain(
      `Claude can't work in a folder like this; ChatGPT can. Rename it in ${FILES[platform]} first, or choose another folder.`,
    );
    expect(byId("locate-choose")?.textContent).toBe(fr("en", "Choose another folder"));
    expect(byId("locate-anyway")?.textContent).toBe(fr("en", "Locate anyway"));
    expect(relocated()).toEqual([]);
    await press("locate-choose");
    expect(transport.only("pick_folder")).toHaveLength(2);
    expect(relocated()).toEqual(["ref-1"]);
    expect(byId("locate-warning")).toBeNull();
  },
);

test.each(["mac", "linux"] as const)(
  "on %s, Locate anyway relocates the matter to the folder with a backslash",
  async (platform) => {
    const { transport, locate, relocated } = await mount(platform, [SLASHED]);
    await locate();
    await press("locate-anyway");
    expect(transport.only("pick_folder")).toHaveLength(1);
    expect(relocated()).toEqual(["ref-0"]);
    expect(byId("locate-warning")).toBeNull();
  },
);

test.each(["mac", "linux"] as const)(
  "on %s, a folder above the one picked has a backslash: the step names it",
  async (platform) => {
    const { locate, relocated } = await mount(platform, ["Documents/Old\\Work/Khan"]);
    await locate();
    const step = byId("locate-warning");
    expect(step?.textContent).toContain(
      'The folder "\u2068Old\\Work\u2069" in this folder\'s path has a backslash (\\).',
    );
    expect(step?.textContent).toContain(
      `Claude can't work in a folder like this; ChatGPT can. Rename that folder in ${FILES[platform]} first, or choose another folder.`,
    );
    await press("locate-anyway");
    expect(relocated()).toEqual(["ref-0"]);
  },
);

test.each([
  ["windows", "D:\\Matters\\Clauseo\\ Test"],
  ["mac", "Documents/Clauseo Test Khan"],
  ["linux", "Documents/Clauseo Test Khan"],
] as const)("on %s, %s relocates at once, with no step", async (platform, display) => {
  const { locate, relocated } = await mount(platform, [display]);
  await locate();
  expect(byId("locate-warning")).toBeNull();
  expect(relocated()).toEqual(["ref-0"]);
});
