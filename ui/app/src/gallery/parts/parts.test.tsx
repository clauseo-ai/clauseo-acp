// @vitest-environment happy-dom
import { settled } from "@clauseo/components/lib/late.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { PartsPage } from "./page.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// happy-dom has no font loading, and the thread waits on the fonts once it lays out: here the faces count as loaded,
// as the thread's own tests have them.
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

let root: Root | undefined;
afterEach(() => {
  act(() => root?.unmount());
  document.body.replaceChildren();
});

test("the parts gallery renders every group without a runtime <style>", async () => {
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  // the gallery shows the parts' lazy pieces (the plan's select, an error's details): they load inside the act
  await act(async () => {
    root?.render(
      <PartsRoot>
        <PartsPage />
      </PartsRoot>,
    );
    await settled();
  });
  expect(container.querySelector("h1")?.textContent).toBe("Clauseo parts");
  const titles = Array.from(container.querySelectorAll("[data-group] > h2"), (h) => h.textContent);
  expect(titles).toEqual(["The room", "Matters", "Versions", "The edge", "The reader"]);
  expect(container.querySelectorAll("h3").length).toBeGreaterThan(0);
  expect(document.querySelectorAll("style").length).toBe(0);
});
