// @vitest-environment happy-dom
// axe-core 4.14.0 over the room's frame, in every theme, language, lite and motion, with a matter and its papers, with
// a matter and none, and with no matter. happy-dom computes no styles, so axe can't judge colour contrast here: that
// is checked by hand in the app, with window.__clauseoAxe() (dev-hooks.ts).
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, expect, test } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { DEFAULT_UI } from "./display-store.ts";
import type { PaperSummary, UiSettings } from "./host-next.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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

const SCENES = {
  "a matter with papers": { matters: true, papers: PAPERS },
  "a matter with none": { matters: true, papers: [] },
  "no matter": { matters: false, papers: undefined },
} as const;

function transportFor(scene: (typeof SCENES)[keyof typeof SCENES]): FakeTransport {
  return new FakeTransport({
    query: () => ({
      matters: scene.matters
        ? [{ matter: uuid(1, 2), name: "Sharma v. State of Delhi", kind: "folder" }]
        : [],
    }),
    ...(scene.papers === undefined
      ? {}
      : {
          next: {
            query: (query) =>
              query === "get_settings"
                ? { settings: { ui: DEFAULT_UI, locked: [] } }
                : { papers: { papers: [...scene.papers], skipped: [], truncated: false } },
          },
        }),
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
            await act(async () => root?.render(<Room transport={transport} />));
            await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
            // the display the case names is the one on <html>
            const html = document.documentElement;
            expect(html.dataset["theme"]).toBe(theme === "dark" ? "night" : "day");
            expect(html.lang).toBe(language);
            expect(html.dataset["render"]).toBe(lite);
            expect(html.dataset["motion"]).toBe(motion);
            expect(store.display().lang).toBe(language);
            expect(host.querySelector("h1")?.textContent, name).not.toBe("");
            const results = await axe.run(document, { resultTypes: ["violations"] });
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
  const transport = transportFor(SCENES["a matter with papers"]);
  connectRoom(transport, {});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  host.querySelector("main")?.append(document.createElement("button"));
  const results = await axe.run(document, { resultTypes: ["violations"] });
  expect(results.violations.map((v) => v.id)).toContain("button-name");
});
