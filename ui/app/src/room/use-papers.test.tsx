// @vitest-environment happy-dom
// usePapers (use-papers.ts): the wall's papers are listed once when a matter opens, again on each `papers_changed` for
// that matter (the host's tracked-changes scan found more), never for another matter's, and one listing at a time:
// changes while one runs are listed once, after it.
import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import type { MatterId } from "../transport/protocol.ts";
import type { AppEvent, AppHost, Papers, PapersHost } from "./host-next.ts";
import type { ListedPapers } from "./open.ts";
import { READ_WAIT_MS, usePapers } from "./use-papers.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const A = "01900000-0000-7000-8000-00000000000a" as MatterId;
const B = "01900000-0000-7000-8000-00000000000b" as MatterId;

/** A listing with one paper whose `tracked` says whether its tile offers Tracked changes. */
const papers = (tracked: boolean): Papers =>
  ({
    papers: [
      {
        file: "01900000-0000-7000-8000-0000000000f1",
        path: "lease.docx",
        size: 10,
        modified: 0,
        tracked,
      },
    ],
    skipped: [],
    unread: [],
    truncated: false,
    place: "local",
  }) as unknown as Papers;

/** A papers host whose answers the test settles, one per call, in any order. */
function fakePapers() {
  const calls: { matter: MatterId; answer: (papers: Papers) => void }[] = [];
  const host: PapersHost = {
    listPapers: (matter) =>
      new Promise<Papers>((resolve) => {
        calls.push({ matter, answer: resolve });
      }),
  };
  return { host, calls };
}

/** An app host the test plays events on. */
function fakeApp() {
  const listeners = new Set<(event: AppEvent) => void>();
  const app: AppHost = {
    subscribeApp: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  const play = (event: AppEvent) => {
    for (const listener of listeners) listener(event);
  };
  return { app, play, listeners };
}

let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

/** Mounts usePapers over the fakes; `seen()` is its latest answer. */
function mount(host: PapersHost, app: AppHost, matter: MatterId) {
  const seen: { latest: ListedPapers } = { latest: null };
  function Probe() {
    const listed = usePapers(host, app, matter);
    useEffect(() => {
      seen.latest = listed;
    }, [listed]);
    return null;
  }
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  act(() => root?.render(<Probe />));
  return { seen: () => seen.latest };
}

const tracked = (listed: ListedPapers) => listed?.papers.papers[0]?.tracked;

test("papers_changed for the open matter lists the papers again", async () => {
  const { host, calls } = fakePapers();
  const { app, play } = fakeApp();
  const probe = mount(host, app, A);
  expect(calls.map((call) => call.matter)).toEqual([A]);
  await act(async () => calls[0]?.answer(papers(false)));
  expect(tracked(probe.seen())).toBe(false);

  act(() => play({ papers_changed: { matter: B } }));
  expect(calls).toHaveLength(1);
  act(() => play({ papers_changed: { matter: A } }));
  expect(calls.map((call) => call.matter)).toEqual([A, A]);
  await act(async () => calls[1]?.answer(papers(true)));
  expect(tracked(probe.seen())).toBe(true);
});

test("one listing at a time: changes while one runs list once more after it", async () => {
  const { host, calls } = fakePapers();
  const { app, play } = fakeApp();
  const probe = mount(host, app, A);
  act(() => {
    play({ papers_changed: { matter: A } });
    play({ papers_changed: { matter: A } });
    window.dispatchEvent(new Event("focus"));
  });
  expect(calls, "none while the first runs").toHaveLength(1);
  await act(async () => calls[0]?.answer(papers(false)));
  expect(calls, "one more for all three").toHaveLength(2);
  expect(tracked(probe.seen())).toBe(false);
  await act(async () => calls[1]?.answer(papers(true)));
  expect(calls, "nothing left to list").toHaveLength(2);
  expect(tracked(probe.seen())).toBe(true);
});

test("unmounting stops listening", () => {
  const { host } = fakePapers();
  const { app, listeners } = fakeApp();
  mount(host, app, A);
  expect(listeners.size).toBe(1);
  act(() => root?.unmount());
  root = null;
  expect(listeners.size).toBe(0);
});

test("a listing that never answers lets the next one go after the wait", async () => {
  vi.useFakeTimers();
  try {
    const { host, calls } = fakePapers();
    const { app, play } = fakeApp();
    const probe = mount(host, app, A);
    act(() => play({ papers_changed: { matter: A } }));
    expect(calls, "the first never answers").toHaveLength(1);
    act(() => {
      vi.advanceTimersByTime(READ_WAIT_MS);
    });
    expect(calls, "the change waiting behind it goes out").toHaveLength(2);
    await act(async () => calls[1]?.answer(papers(true)));
    expect(tracked(probe.seen())).toBe(true);
    await act(async () => calls[0]?.answer(papers(false)));
    expect(tracked(probe.seen()), "the late first answer is not shown").toBe(true);
  } finally {
    vi.useRealTimers();
  }
});
