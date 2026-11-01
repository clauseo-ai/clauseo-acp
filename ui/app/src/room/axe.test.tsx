// @vitest-environment happy-dom
// axe-core 4.14.0 over the room's frame, in every theme, language, lite and motion, with a matter and its papers, with
// a matter and none, with no matter (the empty rail), when list_matters fails, with the rail in every state, and with
// the new matter dialog open, with and without a NextHost. happy-dom computes no styles, so axe can't judge colour contrast here: that
// is checked by hand in the app, with window.__clauseoAxe() (dev-hooks.ts).
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, expect, test } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { MatterSummary } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext, railStates } from "./fake-next.ts";
import type { PaperSummary, UiSettings } from "./host-next.ts";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { Room } from "./room.tsx";
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
};

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
};

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

function transportFor(scene: Scene): FakeTransport {
  const matters: MatterSummary[] = scene.matters === true ? [OPEN, SCRATCH] : [];
  const infos =
    scene.matters === "states"
      ? railStates(OPEN, SCRATCH, Date.now())
      : matters.map((m) => detailed(m));
  return new FakeTransport({
    query: (query) => {
      if (scene.matters === "fails") throw new Error("the host is gone");
      if (typeof query === "object" && query.list_threads !== undefined) return { threads: [] };
      return { matters };
    },
    ...(scene.papers === undefined
      ? {}
      : { next: fakeNext({ matters: () => infos, papers: scene.papers }) }),
  });
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
            await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
            if (scene.dialog === true) {
              await act(async () =>
                host?.querySelector<HTMLButtonElement>('[data-testid="new-matter"]')?.click(),
              );
              await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
              expect(document.querySelector('[role="dialog"]'), name).not.toBeNull();
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
  host.querySelector("main")?.append(document.createElement("button"));
  const results = await axe.run(document, { resultTypes: ["violations"] });
  expect(results.violations.map((v) => v.id)).toContain("button-name");
});
