// DOM helpers for tests that render the window under happy-dom. Not shipped: only tests import it.
import { vi } from "vitest";
import type { Batch } from "../store/batch.ts";

/** happy-dom lays nothing out. Gives a scroller (overflow auto or scroll) a viewport tall enough for every row and
 * every other element a 40 px height, so the virtualized transcript renders all its rows. */
export function stubLayout(): void {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    const tall = /auto|scroll/.test(this.style.overflowY + this.style.overflow);
    const height = tall ? 1_000_000 : 40;
    const rect = { x: 0, y: 0, top: 0, left: 0, width: 800, height, right: 800, bottom: height };
    return { ...rect, toJSON: () => rect };
  });
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(1_000_000);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(800);
}

/** A store `Batch` that runs only when the test calls `run`: one call is one frame. */
export function manualBatch(): { batch: Batch; run: () => void; waiting: () => number } {
  const pending: (() => void)[] = [];
  return {
    batch: (flush) => {
      pending.push(flush);
      return () => {
        const at = pending.indexOf(flush);
        if (at >= 0) pending.splice(at, 1);
      };
    },
    run: () => pending.splice(0).forEach((f) => f()),
    waiting: () => pending.length,
  };
}
