// @vitest-environment happy-dom
// The notice after Clauseo closed unexpectedly (crash-notice.tsx), over a transport that answers `last_exit`: the
// general lines after an unclean exit with nothing recorded, a line per unfinished action by kind and count (with its
// matter's name, never the Scratch matter's), nothing after a clean quit or a refused query, Got it once per run,
// kept across a reload, with focus left in the place the card stood, a restore's Hindi agreeing with बहाली, and after a
// clean quit with an action still running the action alone, with no title.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { HostTransport } from "../transport/host-transport.ts";
import type { InterruptedAction, LastExit, MatterSummary } from "../transport/protocol.ts";
import "@clauseo/components/lib/words-hi.ts";
import { actionLine } from "./crash-lines.ts";
import { CrashNotice } from "./crash-notice.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const MATTER = "00000000-0000-4000-8000-000000000001";
const SCRATCH = "00000000-0000-4000-8000-000000000002";
const THREAD = "00000000-0000-4000-8000-000000000003";
let root: Root | null = null;
let host: HTMLDivElement | null = null;

beforeEach(() => localStorage.clear());
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const MATTERS = [
  { matter: MATTER, name: "Acme lease", kind: "folder" },
  { matter: SCRATCH, name: "Scratch", kind: "scratch" },
] as unknown as MatterSummary[];

const exit = (over: Partial<LastExit> = {}): LastExit => ({
  unclean: true,
  run_started: 1_790_000_000_000,
  ...over,
});

async function settle() {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

/** The notice over a host that answers `last_exit` with `answer`, or refuses when it is an Error. */
async function mount(answer: LastExit | Error) {
  const transport = {
    lastExitWire: true,
    query: async (query: unknown) => {
      expect(query).toBe("last_exit");
      if (answer instanceof Error) throw answer;
      return { last_exit: answer };
    },
  } as unknown as HostTransport;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<CrashNotice transport={transport} matters={MATTERS} />));
  await settle();
  return host;
}

const card = () => host?.querySelector(".crash-notice") ?? null;

test("an unclean exit with nothing recorded says only that the conversations are kept", async () => {
  await mount(exit());
  expect(card()?.getAttribute("aria-label")).toBe("Last session");
  expect(card()?.querySelector(".rail-notice-title")?.textContent).toBe(
    "Clauseo closed unexpectedly.",
  );
  expect(card()?.querySelector(".rail-notice-line")?.textContent).toBe(
    "Your conversations are kept.",
  );
});

test("each unfinished action is named by kind and count, with its matter, then where the copies are", async () => {
  const actions: InterruptedAction[] = [
    { kind: "undo", matter: MATTER, thread: THREAD, files: 3 },
    { kind: "restore", matter: SCRATCH },
    { kind: "accept_changes", matter: MATTER, files: 1 },
  ];
  await mount(exit({ interrupted: actions }));
  expect(card()?.querySelector(".rail-notice-line")?.textContent).toBe(
    "Your conversations are kept. Undo of 3 files in Acme lease may not have finished. A restore may not " +
      "have finished. Accepting tracked changes in Acme lease may not have finished. Versions has the earlier copies.",
  );
});

test("one file, a counted restore and a rejection read in the singular and by name", async () => {
  await mount(
    exit({
      interrupted: [
        { kind: "undo", matter: SCRATCH, files: 1 },
        { kind: "restore", matter: SCRATCH, files: 2 },
        { kind: "reject_changes", matter: SCRATCH, files: 1 },
      ],
    }),
  );
  expect(card()?.querySelector(".rail-notice-line")?.textContent).toBe(
    "Your conversations are kept. Undo of 1 file may not have finished. Restore of 2 files may not have " +
      "finished. Rejecting tracked changes may not have finished. Versions has the earlier copies.",
  );
});

test("a clean quit and a refused query show nothing", async () => {
  await mount(exit({ unclean: false }));
  expect(card()).toBeNull();
  await act(async () => root?.unmount());
  host?.remove();
  await mount(new Error("refused"));
  expect(card()).toBeNull();
});

test("Got it hides it for this run, across a reload; the next unclean exit shows it again", async () => {
  await mount(exit());
  const button = host?.querySelector<HTMLButtonElement>('[data-testid="crash-got-it"]');
  expect(button?.textContent).toBe("Got it");
  await act(async () => button?.click());
  expect(card()).toBeNull();
  await act(async () => root?.unmount());
  host?.remove();
  await mount(exit());
  expect(card()).toBeNull();
  await act(async () => root?.unmount());
  host?.remove();
  await mount(exit({ run_started: 1_790_000_100_000 }));
  expect(card()).not.toBeNull();
});

test("Got it leaves focus in the wall the notice stood in", async () => {
  const transport = {
    lastExitWire: true,
    query: async () => ({ last_exit: exit() }),
  } as unknown as HostTransport;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <aside className="room-pane">
        <CrashNotice transport={transport} matters={MATTERS} />
        <button type="button" data-testid="wall-first">
          A paper
        </button>
      </aside>,
    ),
  );
  await settle();
  expect(card()?.getAttribute("aria-live")).toBe("polite");
  const button = host.querySelector<HTMLButtonElement>('[data-testid="crash-got-it"]');
  await act(async () => button?.click());
  expect(card()).toBeNull();
  expect(document.activeElement?.getAttribute("data-testid")).toBe("wall-first");
});

test("a restore's line in Hindi agrees with बहाली", () => {
  const restore: InterruptedAction = { kind: "restore", matter: MATTER, files: 2 };
  expect(actionLine("hi", restore, MATTERS)).toBe("Acme lease में 2 फ़ाइलों की बहाली शायद पूरी नहीं हुई।");
  expect(actionLine("hi", { ...restore, matter: SCRATCH }, MATTERS)).toBe(
    "2 फ़ाइलों की बहाली शायद पूरी नहीं हुई।",
  );
});

test("after a clean quit with an action still running, the action is named with no title and no crash line", async () => {
  await mount(exit({ unclean: false, interrupted: [{ kind: "undo", matter: MATTER, files: 2 }] }));
  expect(card()?.getAttribute("data-state")).toBe("unfinished");
  expect(card()?.querySelector(".rail-notice-title")).toBeNull();
  const line = card()?.querySelector(".rail-notice-line")?.textContent ?? "";
  expect(line).not.toContain("Your conversations are kept.");
  expect(line).toContain("Acme lease");
  expect(line).toContain("Versions has the earlier copies.");
  expect(card()?.querySelector('[data-testid="crash-got-it"]')).not.toBeNull();
});

test("an unclean exit keeps its title with rows and without", async () => {
  await mount(exit({ interrupted: [{ kind: "undo", matter: MATTER, files: 2 }] }));
  expect(card()?.querySelector(".rail-notice-title")?.textContent).toBe(
    "Clauseo closed unexpectedly.",
  );
  await act(async () => root?.unmount());
  host?.remove();
  await mount(exit());
  expect(card()?.querySelector(".rail-notice-title")?.textContent).toBe(
    "Clauseo closed unexpectedly.",
  );
});
