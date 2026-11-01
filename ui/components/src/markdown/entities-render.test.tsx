// @vitest-environment happy-dom
// The full entity table loads lazily (entities.ts): the component fetches it when the browser is idle after
// its first render, and an answer that met a name the common set does not know re-renders with it.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { characterEntitiesLoaded } from "./entities.ts";
import { Markdown } from "./markdown.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const TEXT = "Plain block.\n\nA rare &ngE; here.\n\nA common &amp; one.\n\nLast.\n";

test("the component fetches the table when idle and re-renders with it", async () => {
  expect(characterEntitiesLoaded()).toBe(false);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Markdown text={TEXT} />));
  expect(host.textContent).toContain("&ngE;");
  await act(async () => {
    await vi.waitFor(() => expect(characterEntitiesLoaded()).toBe(true), { timeout: 5000 });
  });
  expect(host.textContent).toContain("A rare ≧̸ here.");
  expect(host.textContent).toContain("A common & one.");
  act(() => root.unmount());
  host.remove();
});
