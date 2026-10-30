// @vitest-environment happy-dom
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { Gallery } from "./gallery.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;
afterEach(() => {
  act(() => root?.unmount());
  document.body.replaceChildren();
});

test("the gallery renders every section without a runtime <style>", () => {
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  act(() =>
    root?.render(
      <PartsRoot>
        <Gallery />
      </PartsRoot>,
    ),
  );
  expect(container.querySelector("h1")?.textContent).toBe("Clauseo controls");
  const titles = Array.from(container.querySelectorAll("h2"), (h) => h.textContent);
  expect(titles).toContain("Buttons");
  expect(titles).toContain("Icons");
  expect(document.querySelectorAll("style").length).toBe(0);
});
