// @vitest-environment happy-dom
// The renderer's code blocks and the colouring client: a fence asks for tokens once it has closed, never
// while it may still be open, and inline code never asks.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { Markdown } from "../markdown.tsx";
import { createHighlightClient } from "./client.ts";
import { HighlightClientContext } from "./context.ts";
import { fakeFactory } from "./worker-fixture.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
let factory: ReturnType<typeof fakeFactory>;
let client: ReturnType<typeof createHighlightClient>;
beforeEach(() => {
  host = document.createElement("div");
  root = createRoot(host);
  factory = fakeFactory();
  client = createHighlightClient(factory);
});
afterEach(() => {
  act(() => root.unmount());
});

async function show(text: string, streaming: boolean) {
  await act(async () =>
    root.render(
      <HighlightClientContext value={client}>
        <Markdown text={text} streaming={streaming} />
      </HighlightClientContext>,
    ),
  );
}
const asked = () => factory.workers.flatMap((worker) => worker.asked.map((r) => r.code));

test("an open fence while streaming asks nothing; once closed and followed by more, it asks once", async () => {
  await show("Intro with `inline code`.\n\n```ts\nconst a", true);
  await show("Intro with `inline code`.\n\n```ts\nconst a = 1;", true);
  expect(factory.workers).toHaveLength(0);
  expect(host.querySelector("pre")?.getAttribute("data-lang")).toBe("ts");

  await show("Intro with `inline code`.\n\n```ts\nconst a = 1;\n```\n\nAfter", true);
  expect(asked()).toEqual(["const a = 1;"]);

  await act(async () =>
    factory.workers[0]?.answer([
      [
        [2, "const"],
        [0, " a = "],
        [4, "1"],
        [0, ";"],
      ],
    ]),
  );
  const pre = host.querySelector('pre[data-clauseo="code"]');
  expect(pre?.hasAttribute("data-highlighted")).toBe(true);
  expect(pre?.querySelector(".text-code-keyword")?.textContent).toBe("const");
  expect(pre?.textContent).toBe("const a = 1;");
  // Inline code stays a plain <code>, uncoloured.
  expect(host.querySelector("p code")?.outerHTML).toBe("<code>inline code</code>");
});

test("a finished answer colours its blocks, an unclosed last fence included", async () => {
  await show("```python\nprint(1)\n```\n\n```bash\necho hi", false);
  expect(asked()).toEqual(["print(1)"]);
  await act(async () => factory.workers[0]?.answer(null));
  expect(asked()).toEqual(["echo hi"]);
});

test("a closed fence that is still the streaming tail waits for the next block or the turn's end", async () => {
  await show("```json\n{}\n```", true);
  expect(factory.workers).toHaveLength(0);
  await show("```json\n{}\n```", false);
  expect(asked()).toEqual(["{}"]);
});

test("a fence in a language outside the eight, or with none, never starts the worker", async () => {
  await show("```rust\nfn main() {}\n```\n\n```\nplain\n```\n\n    indented", false);
  expect(factory.workers).toHaveLength(0);
  expect(host.querySelectorAll("pre")).toHaveLength(3);
});
