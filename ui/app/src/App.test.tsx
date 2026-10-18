import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { App } from "./App.tsx";
import { NullTransport } from "./transport/host-transport.ts";

test("the app renders one empty #app-root and nothing else", () => {
  expect(renderToStaticMarkup(<App transport={new NullTransport()} />)).toBe(
    '<div id="app-root"></div>',
  );
});
