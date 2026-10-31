// @vitest-environment happy-dom
// A toolbar narrower than its tools wraps them onto another line: at 390 px the gallery's specimen ran 43 px past the
// page while its search field stayed 208 px wide on the one line.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { Toolbar, ToolbarButton } from "./toolbar.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(className?: string) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <Toolbar label="Document" {...(className === undefined ? {} : { className })}>
        <ToolbarButton>Undo</ToolbarButton>
      </Toolbar>,
    ),
  );
  return host.querySelector("[role=toolbar]");
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

test("a toolbar wraps its tools", async () => {
  const bar = await mount();
  expect(bar?.classList.contains("flex-wrap")).toBe(true);
  expect(bar?.classList.contains("flex-nowrap")).toBe(false);
});

test("a toolbar given flex-nowrap keeps its tools on one line", async () => {
  const bar = await mount("flex-nowrap");
  expect(bar?.classList.contains("flex-nowrap")).toBe(true);
  expect(bar?.classList.contains("flex-wrap")).toBe(false);
});
