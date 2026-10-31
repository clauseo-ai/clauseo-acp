import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { App } from "./App.tsx";
import { NullTransport } from "./transport/host-transport.ts";

// ROOT_TAG in scripts/buildtime-lib.mjs: hot-reload timing finds the app by this opening tag.
const ROOT_TAG = '<div id="app-root"';

test("the app renders inside #app-root, whose opening tag buildtime's ROOT_TAG matches", () => {
  const html = renderToStaticMarkup(<App transport={new NullTransport()} />);
  expect(html.startsWith(`${ROOT_TAG}>`)).toBe(true);
  expect(html).toContain('data-testid="new-matter"');
  expect(html).toContain('data-testid="composer"');
});
