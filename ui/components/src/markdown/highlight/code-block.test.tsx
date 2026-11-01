// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { createHighlightClient } from "./client.ts";
import { HighlightedCode } from "./code-block.tsx";
import { fakeFactory } from "./worker-fixture.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
});

const CODE = "const a = 1;\nlet b = 2;";

test("a block whose fence is still open is plain and starts no worker; closing it asks the worker once", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient(factory);
  await act(async () =>
    root.render(<HighlightedCode code="const a" lang="ts" finished={false} client={client} />),
  );
  await act(async () =>
    root.render(<HighlightedCode code={CODE} lang="ts" finished={false} client={client} />),
  );
  expect(factory.workers).toHaveLength(0);
  expect(host.querySelector("code")?.textContent).toBe(CODE);
  expect(host.querySelector("pre")?.hasAttribute("data-highlighted")).toBe(false);

  await act(async () =>
    root.render(<HighlightedCode code={CODE} lang="ts" finished client={client} />),
  );
  expect(factory.workers).toHaveLength(1);
  expect(factory.workers[0]?.asked.map((r) => r.code)).toEqual([CODE]);
  expect(host.querySelector("pre")?.hasAttribute("data-highlighted")).toBe(false);

  await act(async () =>
    factory.workers[0]?.answer([
      [
        [2, "const"],
        [0, " a = "],
        [4, "1"],
        [0, ";"],
      ],
      [
        [2, "let"],
        [0, " b = "],
        [4, "2"],
        [0, ";"],
      ],
    ]),
  );
  const pre = host.querySelector("pre");
  expect(pre?.hasAttribute("data-highlighted")).toBe(true);
  expect(pre?.textContent).toBe(CODE);
  expect(
    [...host.querySelectorAll("code span span")].map((s) => [s.className, s.textContent]),
  ).toEqual([
    ["text-code-keyword", "const"],
    ["text-code-constant", "1"],
    ["text-code-keyword", "let"],
    ["text-code-constant", "2"],
  ]);
  expect(host.querySelector("[style]")).toBeNull();
});

test("a block seen before colours on its first render, without asking again", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient(factory);
  const first = client.highlight("a", "json");
  factory.workers[0]?.answer([[[3, "a"]]]);
  await first;
  await act(async () =>
    root.render(<HighlightedCode code="a" lang="json" finished client={client} />),
  );
  expect(host.querySelector(".text-code-string")?.textContent).toBe("a");
  expect(factory.workers[0]?.asked).toEqual([]);
});

test("a block the worker cannot colour stays plain text", async () => {
  const factory = fakeFactory();
  const client = createHighlightClient(factory);
  await act(async () =>
    root.render(<HighlightedCode code={CODE} lang="sql" finished client={client} />),
  );
  await act(async () => factory.workers[0]?.crash());
  expect(host.querySelector("pre")?.hasAttribute("data-highlighted")).toBe(false);
  expect(host.querySelector("code")?.textContent).toBe(CODE);
  expect(host.querySelectorAll("code span")).toHaveLength(0);
});
