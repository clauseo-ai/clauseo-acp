// @vitest-environment happy-dom
// Each block has its own error boundary: a block whose rendering throws shows its source as plain text,
// and the rest of the answer stays mounted. The model already catches what it can (answer.ts); this is the
// layer for anything thrown while mapping to React, simulated here by a mapper that throws on one block.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterAll, beforeAll, expect, test, vi } from "vitest";
import { Markdown } from "./markdown.tsx";

vi.mock("hast-util-to-jsx-runtime", async (original) => {
  const real = await original<typeof import("hast-util-to-jsx-runtime")>();
  return {
    ...real,
    toJsxRuntime: (...args: Parameters<typeof real.toJsxRuntime>) => {
      if (JSON.stringify(args[0]).includes("boom"))
        throw new RangeError("Maximum call stack size exceeded");
      return real.toJsxRuntime(...args);
    },
  };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeAll(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterAll(() => {
  act(() => root.unmount());
  host.remove();
});

function show(text: string, streaming: boolean) {
  act(() => root.render(<Markdown text={text} streaming={streaming} />));
}

test("a block that throws while rendering shows its source as text; the others render", () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  show("First **one**.\n\nA boom *here*.\n\nLast _one_.\n", false);
  const fallback = host.querySelector('[data-clauseo="markdown-fallback"]');
  expect(fallback?.textContent).toBe("A boom *here*.");
  expect(host.querySelector("strong")?.textContent).toBe("one");
  expect(host.querySelector("em")?.textContent).toBe("one");
  show("First **one**.\n\nA bloom *here*.\n\nLast _one_.\n", false);
  expect(host.querySelector('[data-clauseo="markdown-fallback"]')).toBeNull();
  expect([...host.querySelectorAll("em")].map((em) => em.textContent)).toEqual(["here", "one"]);
  error.mockRestore();
});

test("while streaming, a throwing tail falls back and renders again once it no longer throws", () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  show("Done.\n\nboom", true);
  expect(host.querySelector('[data-clauseo="markdown-fallback"]')?.textContent).toBe("boom");
  expect(host.textContent).toBe("Done.boom");
  show("Done.\n\nboo", true);
  expect(host.querySelector('[data-clauseo="markdown-fallback"]')).toBeNull();
  expect(host.textContent).toBe("Done.boo");
  error.mockRestore();
});
