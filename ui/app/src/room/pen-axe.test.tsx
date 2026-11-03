// @vitest-environment happy-dom
// axe finds nothing on the working line with the pen writing, in day and night (axe-core resolves here, not in
// ui/components; the pen's own frames are tested in ui/components/src/clauseo/pen-mark.test.tsx).
import axe from "axe-core";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "@clauseo/components/lib/display.ts";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { WorkingLine } from "@clauseo/components/clauseo/working-line.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// happy-dom has no SVG geometry
const proto = SVGElement.prototype as unknown as Record<string, unknown>;
proto["getTotalLength"] = () => 1000;
proto["getPointAtLength"] = (k: number) => ({ x: k, y: 0 });

afterEach(() => showDisplay(DEFAULT_DISPLAY));

test.each(["day", "night"] as const)(
  "axe finds nothing on the working line with the pen, in %s",
  async (theme) => {
    showDisplay({ ...DEFAULT_DISPLAY, theme });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () =>
      root.render(
        <RoomScopeProvider>
          <WorkingLine
            state={{
              kind: "wait",
              words: "Reading the complaint.",
              pill: "Expected by 4:32 pm",
              working: true,
            }}
          />
        </RoomScopeProvider>,
      ),
    );
    expect(host.querySelector(".pen-mark")).not.toBeNull();
    const results = await axe.run(host, { resultTypes: ["violations"] });
    expect(results.violations.map((v) => v.id)).toEqual([]);
    await act(async () => root.unmount());
    host.remove();
  },
);
