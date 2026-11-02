// @vitest-environment happy-dom
// A draft's first send (session.ts): the thread create gives is the one the commands go to. Where the conversations'
// emit has opened it already, the session sets nothing more, so the reply's frame has no second commit (#253); where
// the emit hasn't committed yet, the session holds the thread itself.
import { act, createRef, Profiler, useImperativeHandle, useMemo, useState, type Ref } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, expect, test } from "vitest";
import { createViewStore, type ViewStore } from "../../store/store.ts";
import { MATTER, THREAD } from "../../testing/stream-script.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { HostCommand, ThreadId } from "../../transport/protocol.ts";
import { loadThread } from "./chunk.ts";
import { useThreadSession, type ConversationLink } from "./session.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Handle = {
  ask: (text: string) => Promise<boolean>;
  thread: ThreadId | null;
  /** The conversations' emit: the room renders with the link's thread set. */
  emit: (thread: ThreadId) => void;
};

function Host({
  handle,
  transport,
  store,
  create,
}: {
  handle: Ref<Handle>;
  transport: FakeTransport;
  store: ViewStore;
  create: ConversationLink["create"];
}) {
  const [linked, setLinked] = useState<ThreadId | null>(null);
  const link = useMemo(
    () => ({ thread: linked, create, sent: () => {}, refused: null }),
    [linked, create],
  );
  const session = useThreadSession(transport, store, MATTER, link, "draft:1");
  useImperativeHandle(
    handle,
    () => ({ ask: session.ask, thread: session.thread, emit: setLinked }),
    [session.ask, session.thread],
  );
  return null;
}

let unmount: (() => void) | null = null;
afterEach(() => {
  unmount?.();
  unmount = null;
});
// the thread opens only once its chunk is in (chunk.ts)
beforeAll(async () => void (await loadThread()));

/** Mounts a session on a draft whose create waits for `made`, and whose send_prompt never answers. */
function mount() {
  let made: (thread: ThreadId) => void = () => {};
  const create = () => new Promise<ThreadId>((done) => (made = done));
  // send_prompt waits for ever: what follows the create is the session's alone
  const sent: HostCommand[] = [];
  const transport = new FakeTransport({
    dispatch: (envelope) => {
      sent.push(envelope.command);
      return new Promise(() => {});
    },
  });
  const store = createViewStore(transport, { batch: () => () => () => {} });
  const handle = createRef<Handle>();
  let commits = 0;
  const root = createRoot(document.createElement("div"));
  act(() =>
    root.render(
      <Profiler id="session" onRender={() => (commits += 1)}>
        <Host handle={handle} transport={transport} store={store} create={create} />
      </Profiler>,
    ),
  );
  unmount = () => act(() => root.unmount());
  return {
    handle: () => handle.current!,
    made: (thread: ThreadId) => made(thread),
    commits: () => commits,
    sent: () => sent,
  };
}

test("a first send whose thread the conversations' emit opened already makes no commit of its own (#253)", async () => {
  const r = mount();
  act(() => void r.handle().ask("q"));
  await act(async () => r.handle().emit(THREAD));
  const before = r.commits();
  await act(async () => r.made(THREAD));
  expect(r.commits()).toBe(before);
  expect(r.handle().thread).toBe(THREAD);
  expect(r.sent()).toEqual([
    { send_prompt: { thread: THREAD, content: [{ text: { text: "q" } }] } },
  ]);
});

test("a first send whose emit hasn't committed yet holds the thread itself", async () => {
  const r = mount();
  act(() => void r.handle().ask("q"));
  const before = r.commits();
  await act(async () => r.made(THREAD));
  expect(r.commits()).toBe(before + 1);
  expect(r.handle().thread).toBe(THREAD);
  expect(r.sent()).toEqual([
    { send_prompt: { thread: THREAD, content: [{ text: { text: "q" } }] } },
  ]);
});
