// @vitest-environment happy-dom
// The tool line's review cells, rendered as the review renders them (in StrictMode): each draws its tool line once the
// line's chunk is in, with nothing on the console. React logs an error there for a store read that gives a new value
// each time (useSyncExternalStore's "The result of getSnapshot should be cached"), which the cells once crashed on.
import { settled } from "@clauseo/components/lib/late.ts";
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { UET } from "./registry-uet.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let unmount: (() => Promise<void>) | null = null;
afterEach(async () => {
  await unmount?.();
  unmount = null;
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

test.each(UET.map((e) => e.id))(
  "%s draws its tool line with nothing on the console",
  async (id) => {
    const entry = UET.find((e) => e.id === id)!;
    const errors = vi.spyOn(console, "error");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    unmount = async () => {
      await act(async () => root.unmount());
      host.remove();
    };
    await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
    for (let i = 0; i < 20 && host.querySelector(".room-status-tools") === null; i += 1)
      await act(async () => {
        await settled();
        await new Promise((resolve) => setTimeout(resolve, 20));
      });
    expect(host.querySelector(".room-status-tools")).not.toBeNull();
    expect(errors).not.toHaveBeenCalled();
  },
);
