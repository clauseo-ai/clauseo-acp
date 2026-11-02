// @vitest-environment happy-dom
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { App } from "./App.tsx";
import { NullTransport } from "./transport/host-transport.ts";

// ROOT_TAG in scripts/buildtime-lib.mjs: hot-reload timing finds the app by this opening tag.
const ROOT_TAG = '<div id="app-root"';

test("the app mounts the room inside #app-root, whose opening tag buildtime's ROOT_TAG matches", () => {
  const html = renderToStaticMarkup(<App transport={new NullTransport()} />);
  expect(html.startsWith(`${ROOT_TAG}>`)).toBe(true);
  expect(html).toContain('class="room-window"');
  expect(html).toContain("room-rail-toggle");
  // the title's heading waits for list_matters: never an empty one
  expect(html).not.toContain("<h1");
  // the rail's New matter and the room's composer (room/thread/) stand from the first render, before list_matters
  // answers: the perf module's `rendered` mark waits for both (src/perf/index.ts)
  expect(html).toContain('data-testid="new-matter"');
  expect(html).toContain('data-testid="composer"');
  expect(html).toContain('class="room-composer"');
});

// U5 retired R12's internal window with its build switch: there is one app.
test("the app is the room alone: no internal window and no build switch left in its code", () => {
  const html = renderToStaticMarkup(<App transport={new NullTransport()} />);
  // R12's internal window labelled its field "Prompt"
  expect(html).not.toContain('aria-label="Prompt"');
  const src = import.meta.dirname;
  expect(existsSync(join(src, "internal"))).toBe(false);
  const files = [
    ...readdirSync(src, { recursive: true }).map((f) => join(src, String(f))),
    join(src, "../vite.config.ts"),
  ].filter((f) => /\.(ts|tsx|mjs)$/.test(f) && !f.endsWith("App.test.tsx"));
  expect(files.length).toBeGreaterThan(50);
  for (const file of files)
    expect(readFileSync(file, "utf8"), file).not.toContain("VITE_CLAUSEO_WINDOW");
});
