// @vitest-environment happy-dom
// useRefused (session.ts): a set to the code already held is never made, so an Ask with nothing refused adds no render
// of Work beside its reply's (#253).
import { act, createRef, Profiler, useImperativeHandle, type Ref } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import type { ErrorCode } from "../../transport/protocol.ts";
import { useRefused } from "./session.ts";

// every state set the hook makes, counted: on an idle component React would drop a set to the same value before any
// render, so the renders alone can't show the set it keeps queued once another lane's update waits (#253)
const sets = vi.hoisted(() => ({ n: 0 }));
vi.mock("react", async (importOriginal) => {
  const react = await importOriginal<typeof import("react")>();
  const useState = ((initial: unknown) => {
    const [value, set] = react.useState(initial);
    return [
      value,
      (next: unknown) => {
        sets.n += 1;
        set(next);
      },
    ];
  }) as typeof react.useState;
  return { ...react, useState };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let unmount: (() => void) | null = null;
afterEach(() => {
  unmount?.();
  unmount = null;
});

type Handle = { refuse: (code: ErrorCode | null) => void; held: ErrorCode | null };

function Probe({ handle }: { handle: Ref<Handle> }) {
  const [held, refuse] = useRefused();
  useImperativeHandle(handle, () => ({ refuse, held }), [held, refuse]);
  return null;
}

/** Renders useRefused, counting its commits; gives its setter and what it holds. */
function mount() {
  let renders = 0;
  const handle = createRef<Handle>();
  const root = createRoot(document.createElement("div"));
  act(() =>
    root.render(
      <Profiler id="refused" onRender={() => (renders += 1)}>
        <Probe handle={handle} />
      </Profiler>,
    ),
  );
  unmount = () => act(() => root.unmount());
  return {
    refuse: (code: ErrorCode | null) => handle.current?.refuse(code),
    renders: () => renders,
    held: () => handle.current?.held,
  };
}

test("refuse(null) with nothing refused makes no set and schedules no render (an Ask's click: #253)", () => {
  const r = mount();
  sets.n = 0;
  expect(r.renders()).toBe(1);
  act(() => r.refuse(null));
  expect(sets.n).toBe(0);
  expect(r.renders()).toBe(1);
});

test("refuse sets a new code once, the same code again schedules nothing, and null clears it", () => {
  const r = mount();
  act(() => r.refuse("thread.turn_running"));
  expect(r.held()).toBe("thread.turn_running");
  const after = r.renders();
  sets.n = 0;
  act(() => r.refuse("thread.turn_running"));
  expect(sets.n).toBe(0);
  expect(r.renders()).toBe(after);
  act(() => r.refuse(null));
  expect(r.held()).toBeNull();
  expect(r.renders()).toBe(after + 1);
});
