// @vitest-environment happy-dom
// When the full entity table arrives (entities.ts), only an answer that met a name the common set does not know
// renders again; every other answer stays as it is, so the table's arrival adds no commit to a streaming frame
// (private issue #253). Its own file: the table loads once per module graph.
import { act, Profiler, type ProfilerOnRenderCallback } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { characterEntitiesLoaded } from "./entities.ts";
import { Markdown } from "./markdown.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("the table's arrival renders the answer that missed a name once, and no other answer", async () => {
  expect(characterEntitiesLoaded()).toBe(false);
  const renders = new Map<string, number>();
  const count: ProfilerOnRenderCallback = (id) => renders.set(id, (renders.get(id) ?? 0) + 1);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <>
        <Profiler id="missed" onRender={count}>
          <Markdown text={"A rare &ngE; here.\n"} streaming />
        </Profiler>
        <Profiler id="common" onRender={count}>
          <Markdown text={"A common &amp; one.\n"} streaming />
        </Profiler>
        <Profiler id="plain" onRender={count}>
          <Markdown text={"No reference at all.\n"} streaming />
        </Profiler>
      </>,
    ),
  );
  expect(host.textContent).toContain("&ngE;");
  const before = new Map(renders);
  await act(async () => {
    await vi.waitFor(() => expect(characterEntitiesLoaded()).toBe(true), { timeout: 5000 });
  });
  expect(host.textContent).toContain("A rare ≧̸ here.");
  expect(renders.get("missed")).toBe(before.get("missed")! + 1);
  expect(renders.get("common")).toBe(before.get("common"));
  expect(renders.get("plain")).toBe(before.get("plain"));
  act(() => root.unmount());
  host.remove();
});
