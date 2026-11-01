// @vitest-environment happy-dom
// The scroll-follow stall, in the shape of the macOS scoreboard play that found it (#106): the person scrolls up while
// the answer grows, then scrolls back down by as much. They land short of an end that moved meanwhile, outside
// LegendList's re-follow threshold, so the list stops following and the turn's last rows stay out of sight. The window
// must offer a way back to following.
//
// What this test covers: the window's side only. It tests a model of LegendList's follow rule, not LegendList: the mock
// below stands in for the list and copies its rule from LegendList 3.6 (react.mjs, the
// isWithinMaintainScrollAtEndThreshold update): it follows a growing end only while the scroll is within 10% of the
// viewport of it, and `scrollToEnd` lands on the end. happy-dom lays nothing out, and the real list's scroll state does
// not track a scroll there. That the real list sends this signal and that `scrollToEnd({ animated: false })` makes it
// follow again was checked by hand in headless Chromium, with the issue's repro page (the real App, store and list on a
// fake transport). A committed browser test is a follow-up: ui/e2e was out of scope for this change.
import { act, useImperativeHandle, useLayoutEffect, type ReactNode, type Ref } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { ViewPatchBody } from "../transport/protocol.ts";
import {
  batch,
  event,
  header,
  reset,
  STREAM,
  toolCall,
  upsert,
  userMessage,
} from "../testing/stream-script.ts";
import { manualBatch, stubLayout } from "../testing/dom.ts";
import { ThreadContext } from "./session.ts";
import { Transcript } from "./transcript.tsx";
import { JUMP_TO_LATEST } from "./words.ts";

const VIEWPORT = 680;
const ROW = 40;

/** The model list's geometry, which the test scrolls as a person would. */
const list = {
  scroll: 0,
  content: 0,
  listeners: new Set<(within: boolean) => void>(),
  within: true,
  fromEnd(): number {
    return Math.max(0, this.content - VIEWPORT - this.scroll);
  },
  update(): void {
    const within = this.fromEnd() <= 0.1 * VIEWPORT;
    if (within === this.within) return;
    this.within = within;
    for (const listener of this.listeners) listener(within);
  },
  grow(rows: number): void {
    const following = this.within;
    this.content = rows * ROW;
    if (following) this.scroll = Math.max(0, this.content - VIEWPORT);
    this.update();
  },
  scrollBy(px: number): void {
    this.scroll = Math.min(Math.max(0, this.scroll + px), Math.max(0, this.content - VIEWPORT));
    this.update();
  },
};

vi.mock("@legendapp/list/react", () => ({
  LegendList(props: {
    ref: Ref<unknown>;
    data: readonly string[];
    renderItem: (info: { item: string }) => ReactNode;
  }) {
    useImperativeHandle(props.ref, () => ({
      getState: () => ({
        isWithinMaintainScrollAtEndThreshold: list.within,
        listen: (_: string, listener: (within: boolean) => void) => {
          list.listeners.add(listener);
          return () => list.listeners.delete(listener);
        },
      }),
      scrollToEnd: async () => {
        list.scroll = Math.max(0, list.content - VIEWPORT);
        list.update();
      },
    }));
    useLayoutEffect(() => list.grow(props.data.length));
    return <div>{props.data.map((item) => props.renderItem({ item }))}</div>;
  },
}));

beforeAll(stubLayout);

let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

test("scrolled up and back down while the answer grew, the person can jump to the end and the list follows again", async () => {
  const transport = new FakeTransport();
  const manual = manualBatch();
  const store = createViewStore(transport, { batch: () => manual.batch });
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  const actions = { thread: "t", stream: STREAM, answer: () => {}, undo: () => {} };
  act(() =>
    root!.render(
      <StoreContext value={store}>
        <ThreadContext value={actions}>
          <Transcript stream={STREAM} />
        </ThreadContext>
      </StoreContext>,
    ),
  );
  const release = store.watch(STREAM);
  let cursor = 1;
  const send = async (bodies: ViewPatchBody[]) => {
    transport.deliver(JSON.stringify(event(batch(cursor, bodies))));
    cursor += bodies.length;
    await act(async () => manual.run());
  };
  let order = 10;
  const more = (n: number) =>
    send(Array.from({ length: n }, () => upsert(toolCall(order, order++, "completed"))));
  const jump = () => container.querySelector<HTMLElement>('[data-testid="jump-to-latest"]');

  await send([reset([userMessage(5, 1, "Review the MSA.")], header("running"))]);
  await more(60);
  expect(list.fromEnd()).toBe(0);
  expect(jump()).toBeNull();

  // Autoplay's scroll: 30 frames up and 30 down, 40 px each, while the rows keep coming.
  for (let i = 0; i < 30; i++) await act(async () => list.scrollBy(-40));
  await more(30);
  for (let i = 0; i < 30; i++) await act(async () => list.scrollBy(40));
  await more(10);
  expect(list.fromEnd()).toBe(40 * ROW);
  expect(jump()?.textContent).toBe(JUMP_TO_LATEST);

  await act(async () => jump()!.click());
  expect(list.fromEnd()).toBe(0);
  expect(jump()).toBeNull();
  // The button left with the click; focus is on the log, not lost to the page.
  expect(document.activeElement).toBe(container.querySelector('[data-testid="transcript"]'));
  await more(10);
  expect(list.fromEnd()).toBe(0);
  release();
});
