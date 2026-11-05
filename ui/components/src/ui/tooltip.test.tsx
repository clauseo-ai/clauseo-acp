// @vitest-environment happy-dom
// The app's tooltip stands above every plane that could cover it: it mounts in the page's layer (the end of the body
// without a LayerProvider), outside the box its trigger sits in, and its positioner takes the floating layer's place,
// over the work surface (z 2) and the folded rail's panel (z 10). RO-22's words were cut by both.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { Tooltip, TooltipProvider } from "./tooltip.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

test("a tooltip opened by the keyboard mounts outside its trigger's clipping box, in the floating layer", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <nav className="panel" style={{ overflow: "hidden", position: "absolute", zIndex: 10 }}>
          <Tooltip content="Searches titles, paper names and conversations.">
            <button type="button" aria-label="What Find covers">
              i
            </button>
          </Tooltip>
        </nav>
      </TooltipProvider>,
    ),
  );
  const button = host.querySelector("button");
  await act(async () => {
    button?.focus();
    button?.dispatchEvent(new FocusEvent("focus"));
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  // the positioner, in the portal at the body's end
  const positioner = document.querySelector<HTMLElement>("[data-base-ui-portal] > [data-side]");
  expect(positioner?.textContent).toBe("Searches titles, paper names and conversations.");
  expect(host.contains(positioner)).toBe(false);
  expect(positioner?.classList.contains("z-30")).toBe(true);
});
