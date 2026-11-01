// @vitest-environment happy-dom
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, expect, test, vi } from "vitest";
import { App } from "./App.tsx";
import { NullTransport } from "./transport/host-transport.ts";

// ROOT_TAG in scripts/buildtime-lib.mjs: hot-reload timing finds the app by this opening tag.
const ROOT_TAG = '<div id="app-root"';

afterEach(() => {
  vi.unstubAllEnvs();
  vi.resetModules();
});

test("the app renders inside #app-root, whose opening tag buildtime's ROOT_TAG matches", () => {
  const html = renderToStaticMarkup(<App transport={new NullTransport()} />);
  expect(html.startsWith(`${ROOT_TAG}>`)).toBe(true);
  expect(html).toContain('data-testid="new-matter"');
  expect(html).toContain('data-testid="composer"');
});

// The switch is App.tsx's compile-time `import.meta.env.VITE_CLAUSEO_WINDOW`: each case loads App.tsx afresh under it.
async function appUnder(window: string | undefined) {
  vi.resetModules();
  if (window !== undefined) vi.stubEnv("VITE_CLAUSEO_WINDOW", window);
  return (await import("./App.tsx")).App;
}

test("with VITE_CLAUSEO_WINDOW unset the app mounts R12's internal window, not the room", async () => {
  const Fresh = await appUnder(undefined);
  const html = renderToStaticMarkup(<Fresh transport={new NullTransport()} />);
  expect(html.startsWith(`${ROOT_TAG}>`)).toBe(true);
  expect(html).toContain('data-testid="new-matter"');
  expect(html).not.toContain("room-window");
});

test("with VITE_CLAUSEO_WINDOW=room the app mounts the room inside #app-root, not the internal window", async () => {
  const Fresh = await appUnder("room");
  const html = renderToStaticMarkup(<Fresh transport={new NullTransport()} />);
  expect(html.startsWith(`${ROOT_TAG}>`)).toBe(true);
  expect(html).toContain('class="room-window"');
  expect(html).toContain("room-rail-toggle");
  // the title's heading waits for list_matters: never an empty one
  expect(html).not.toContain("<h1");
  // the rail's New matter stands from the first render, before list_matters answers
  expect(html).toContain('data-testid="new-matter"');
  // the room's own composer (room/thread/), not the internal window's
  expect(html).toContain('class="room-composer"');
  expect(html).not.toContain('aria-label="Prompt"');
});

test("any other value of VITE_CLAUSEO_WINDOW mounts the internal window", async () => {
  const Fresh = await appUnder("Room");
  const html = renderToStaticMarkup(<Fresh transport={new NullTransport()} />);
  expect(html).toContain('data-testid="new-matter"');
  expect(html).not.toContain("room-window");
});
