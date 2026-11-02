// @vitest-environment happy-dom
// axe-core 4.14.0 over the room's frame, in every theme, language, lite and motion, with a matter and its papers, with
// a matter and none, with no matter (the empty rail), when list_matters fails, with the rail in every state, and with
// the new matter dialog open, with and without a NextHost, and with the agent's permission requests in the thread: a
// pending prompt on a paper offering all four answers, one answered once and done, a command's, which asks, and one confirming: Always for this matter
// pressed and held while the OS asks, its answers waiting. happy-dom computes no styles, so axe can't judge colour contrast here: that
// is checked by hand in the app, with window.__clauseoAxe() (dev-hooks.ts). And U7's wait over the composer while
// another conversation of the matter runs, with Stop the other and Queue.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeAll, expect, test } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { MatterSummary, ViewEntry } from "../transport/protocol.ts";
import {
  batch,
  chosen,
  commandCall,
  editCall,
  event,
  header,
  MATTER,
  permission,
  reset,
  THREAD,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import { detailed, fakeNext, railStates } from "./fake-next.ts";
import type { PaperSummary, UiSettings } from "./host-next.ts";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { Room } from "./room.tsx";
import { loadThread } from "./thread/chunk.ts";
import { stubFonts } from "./thread/testing.ts";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;

beforeAll(() => {
  // the page's own title, as index.html gives it
  const html = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../index.html"),
    "utf8",
  );
  document.title = /<title>([^<]*)<\/title>/.exec(html)?.[1] ?? "";
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const PAPERS: PaperSummary[] = ["Notice to the bank.pdf", "उत्तर.docx", "Board minutes.pdf"].map(
  (name, i) => ({ file: uuid(i + 1, 8), path: name, size: 1000, modified: 1_730_000_000_000 }),
);

type Scene = {
  matters: boolean | "fails" | "states";
  papers: readonly PaperSummary[] | undefined;
  /** New matter, pressed: the dialog open over the room. */
  dialog?: boolean;
  /** The open matter's one thread, with these entries, waiting on the lawyer. */
  thread?: ViewEntry[];
  /** Always for this matter, pressed and held: the OS's confirm line up, the answers waiting. */
  confirming?: boolean;
  /** The open thread idle while another conversation of its matter runs: the wait over the composer (U7). */
  wait?: boolean;
};

const ASKED = userMessage(2, 1, "Tighten the notice to the bank.");

const SCENES: Record<string, Scene> = {
  "a matter with papers": { matters: true, papers: PAPERS },
  "a matter with none": { matters: true, papers: [] },
  // the empty rail too: "No matters yet" and "Add a matter's folder."
  "no matter": { matters: false, papers: undefined },
  // list_matters rejects: no heading, and nothing else in the title's place
  failed: { matters: "fails", papers: undefined },
  // every rail state, every date group, Pinned and the scratch matter (fake-next.ts's railStates)
  "the rail in every state": { matters: "states", papers: PAPERS },
  "the new matter dialog": { matters: "states", papers: PAPERS, dialog: true },
  "the new matter dialog without a NextHost": { matters: true, papers: undefined, dialog: true },
  // a prompt on one paper inside the matter, all four answers offered
  "a pending permission": {
    matters: true,
    papers: undefined,
    thread: [ASKED, editCall(3, 1, "pending"), permission(4, 1)],
  },
  // answered once, the call completed: the folded line
  "an answered permission": {
    matters: true,
    papers: undefined,
    thread: [ASKED, editCall(3, 1, "completed"), permission(4, 1, chosen("allow_once"))],
  },
  // a command, no paper: "Clauseo asks before going on." and the title beneath
  "a command's permission": {
    matters: true,
    papers: undefined,
    thread: [ASKED, commandCall(3, 2, "pending"), permission(4, 2)],
  },
  // Always for this matter pressed, its answer held while the OS asks the lawyer to confirm
  "a permission confirming": {
    matters: true,
    papers: undefined,
    thread: [ASKED, editCall(3, 1, "pending"), permission(4, 1)],
    confirming: true,
  },
  // another conversation of the matter runs: the wait over the composer, with Stop the other and Queue
  "another conversation holding the matter": {
    matters: true,
    papers: undefined,
    thread: [ASKED],
    wait: true,
  },
};

/** The other conversation for the wait: its id sorts below THREAD's, so THREAD opens. */
const OTHER = uuid(0, 1);

/** The room as App mounts it: inside the page's view store. */
const inStore = (transport: FakeTransport) => (
  <StoreContext value={createViewStore(transport, { batch: () => frameBatch })}>
    <Room transport={transport} />
  </StoreContext>
);

const OPEN: MatterSummary = {
  matter: uuid(1, 2),
  name: "Sharma v. State of Delhi",
  kind: "folder",
};
const SCRATCH: MatterSummary = { matter: uuid(99, 2), name: "Scratch", kind: "scratch" };
/** The matter the scripted thread belongs to (stream-script.ts's MATTER). */
const THE_MATTER: MatterSummary = { ...OPEN, matter: MATTER };

function transportFor(scene: Scene): FakeTransport {
  const matters: MatterSummary[] =
    scene.matters !== true ? [] : scene.thread === undefined ? [OPEN, SCRATCH] : [THE_MATTER];
  const infos =
    scene.matters === "states"
      ? railStates(OPEN, SCRATCH, Date.now())
      : matters.map((m) => detailed(m));
  const thread = scene.thread;
  const transport: FakeTransport = new FakeTransport({
    // the confirming line names the OS's dialog, which Linux has none of
    ...(scene.confirming === true ? { platform: "mac" as const } : {}),
    query: (query) => {
      if (scene.matters === "fails") throw new Error("the host is gone");
      if (typeof query === "object" && query.list_threads !== undefined)
        return {
          threads:
            thread === undefined
              ? []
              : scene.wait === true
                ? [
                    { thread: THREAD, matter: MATTER, agent: "claude-acp", status: "idle" },
                    {
                      thread: OTHER,
                      matter: MATTER,
                      agent: "claude-acp",
                      status: "running",
                      title: "Draft petition",
                    },
                  ]
                : [{ thread: THREAD, matter: MATTER, agent: "claude-acp", status: "needs_you" }],
        };
      return { matters };
    },
    ...(thread === undefined
      ? {}
      : {
          subscribe: (stream) =>
            queueMicrotask(() =>
              transport.deliver(
                event(
                  batch(
                    1,
                    [reset(thread, header(scene.wait === true ? "idle" : "needs_you"))],
                    undefined,
                    stream,
                  ),
                ),
              ),
            ),
        }),
    ...(scene.papers === undefined
      ? {}
      : { next: fakeNext({ matters: () => infos, papers: scene.papers }) }),
  });
  if (scene.confirming === true) transport.answerPermission = () => new Promise<void>(() => {});
  return transport;
}

const THEMES = ["light", "dark"] as const;
const LANGS = ["en", "hi"] as const;
const LITES = ["full", "lite"] as const;
const MOTIONS = ["full", "reduced"] as const;

for (const theme of THEMES) {
  for (const language of LANGS) {
    for (const lite of LITES) {
      for (const motion of MOTIONS) {
        const display: Partial<UiSettings> = { theme, language, lite, motion };
        test(`axe finds no violation: ${theme}, ${language}, lite ${lite}, motion ${motion}`, async () => {
          for (const [name, scene] of Object.entries(SCENES)) {
            const transport = transportFor(scene);
            const store = connectRoom(transport, display);
            host = document.createElement("div");
            document.body.append(host);
            root = createRoot(host);
            await act(async () => root?.render(inStore(transport)));
            // the host's answers, and the thread's chunk (late.ts), so axe checks the room as it stands
            await act(async () => {
              await new Promise((resolve) => setTimeout(resolve, 0));
              await settled();
            });
            if (scene.dialog === true) {
              await act(async () =>
                host?.querySelector<HTMLButtonElement>('[data-testid="new-matter"]')?.click(),
              );
              await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
              expect(document.querySelector('[role="dialog"]'), name).not.toBeNull();
            }
            if (scene.wait === true) {
              await act(async () => void (await loadThread()));
              await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
              const wait = host.querySelector('[data-testid="other-wait"]');
              expect(wait, name).not.toBeNull();
              expect(wait?.querySelector('[data-testid="stop-other"]'), name).not.toBeNull();
              expect(wait?.querySelector('[data-testid="queue"]'), name).not.toBeNull();
            } else if (scene.thread !== undefined) {
              await act(async () => void (await loadThread()));
              await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
              const rows = [...host.querySelectorAll<HTMLElement>('[data-testid="permission"]')];
              expect(
                rows.map((r) => [r.dataset["state"], r.dataset["answer"] ?? null]),
                name,
              ).toEqual([
                scene.thread.some(
                  (e) => e.permission !== undefined && e.permission.state !== "pending",
                )
                  ? ["resolved", "allow_once"]
                  : ["pending", null],
              ]);
              if (rows[0]?.dataset["state"] === "pending")
                expect(host.querySelectorAll('[data-testid^="permission-"]'), name).toHaveLength(4);
              if (scene.confirming === true) {
                await act(async () =>
                  host
                    ?.querySelector<HTMLButtonElement>('[data-testid="permission-allow_always"]')
                    ?.click(),
                );
                expect(rows[0]?.dataset["confirming"], name).toBeDefined();
                expect(rows[0]?.querySelector(".perm-confirming"), name).not.toBeNull();
                // the answers wait, dimmed and held (aria-disabled, never inert), while the OS asks
                expect(rows[0]?.querySelector(".perm-actions[inert]"), name).toBeNull();
                expect(rows[0]?.querySelector('[aria-disabled="true"]'), name).not.toBeNull();
              }
              // the command's prompt asks, with its title beneath
              if (scene.thread.some((e) => e.tool_call?.kind === "execute"))
                expect(rows[0]?.textContent, name).toContain("Run ls papers");
            }
            if (scene.matters === "states")
              expect(host.querySelectorAll('[data-testid="matter-row"]'), name).toHaveLength(13);
            // the display the case names is the one on <html>
            const html = document.documentElement;
            expect(html.dataset["theme"]).toBe(theme === "dark" ? "night" : "day");
            expect(html.lang).toBe(language);
            expect(html.dataset["render"]).toBe(lite);
            expect(html.dataset["motion"]).toBe(motion);
            expect(store.display().lang).toBe(language);
            // a heading with the title, or after a failure none: never an empty one
            const heading = host.querySelector("h1");
            if (scene.matters === "fails") expect(heading, name).toBeNull();
            else expect(heading?.textContent, name).not.toBe("");
            // after a failure the page has no h1 until the failure's words (QU.8) take the title's place: axe's
            // page-has-heading-one, a best practice and no WCAG rule, is the one rule that scene leaves off
            const results = await axe.run(document, {
              resultTypes: ["violations"],
              ...(scene.matters === "fails"
                ? { rules: { "page-has-heading-one": { enabled: false } } }
                : {}),
            });
            const found = results.violations.map(
              (v) => `${name}: ${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
            );
            expect(found).toEqual([]);
            await act(async () => root?.unmount());
            host.remove();
          }
          root = null;
          host = null;
        });
      }
    }
  }
}

test("axe in this environment finds a violation where there is one: a button with no name in the room", async () => {
  const transport = transportFor({ matters: true, papers: PAPERS });
  connectRoom(transport, {});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(inStore(transport)));
  await act(settled);
  host.querySelector("main")?.append(document.createElement("button"));
  const results = await axe.run(document, { resultTypes: ["violations"] });
  expect(results.violations.map((v) => v.id)).toContain("button-name");
});
