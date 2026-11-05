// The rig for U16a's rail tests (rail-needs-you.test.tsx): four matters (one open, one waiting on a
// permission, one with an answer ready, one pinned with a question), the scratch matter, each matter's threads as the
// host's first frames give them, and the room mounted over FakeTransport.
import { act, createElement, Profiler } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { MatterSummary } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { MatterInfo, ThreadInfo } from "./host-next.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

/** Tuesday 3 November 2026, 5 pm UTC. */
export const NOW = Date.UTC(2026, 10, 3, 17, 0);

export const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
export const B: MatterSummary = {
  matter: uuid(2, 2),
  name: "Sharma v. State of Delhi",
  kind: "folder",
};
export const C: MatterSummary = {
  matter: uuid(3, 2),
  name: "Rao v. Union of India",
  kind: "folder",
};
export const D: MatterSummary = {
  matter: uuid(4, 2),
  name: "Desai v. Municipal Corporation",
  kind: "folder",
};
export const SCRATCH: MatterSummary = { matter: uuid(9, 2), name: "Scratch", kind: "scratch" };

export const thread = (
  n: number,
  matter: MatterSummary,
  over: Partial<ThreadInfo> = {},
): ThreadInfo => ({
  thread: uuid(n, 3),
  matter: matter.matter,
  title: `Thread ${n}`,
  agent: "claude-code",
  status: "idle",
  seen: true,
  last_activity: NOW - n * 60_000,
  ...over,
});

export const T_ASK = thread(1, B, { title: "Reply to the notice", status: "needs_you" });
export const T_READY = thread(2, C, {
  title: "Counterclaim, limitation",
  seen: false,
  last_stop: { reason: "end_turn", at: NOW - 600_000 },
});
export const T_PIN = thread(3, D, { title: "Show cause reply", status: "needs_you" });
export const T_A = thread(4, A, { title: "Bail conditions" });

export const MATTERS = (): MatterInfo[] => [
  detailed(A, { last_opened: NOW }),
  detailed(B, { status: "needs_permission" }),
  detailed(C, { status: "ready" }),
  detailed(D, { pinned: true, status: "needs_answer" }),
  detailed(SCRATCH),
];
export const LISTS = new Map<string, ThreadInfo[]>([
  [A.matter, [T_A]],
  [B.matter, [T_ASK]],
  [C.matter, [T_READY]],
  [D.matter, [T_PIN]],
  [SCRATCH.matter, [thread(5, SCRATCH, { title: "Cheque bounce by email" })]],
]);

/** The room over the rig's host; with `onRender`, under a Profiler that hears each commit's phase. Settles the host's
 * answers unless `settled` is false (a test with fake timers runs them itself). */
export async function mount(onRender?: (phase: string) => void, settled = true) {
  const transport = new FakeTransport({
    next: fakeNext({ matters: MATTERS, threads: (m) => LISTS.get(m) ?? [], frames: true }),
  });
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const room = createElement(Room, { transport });
  await act(async () =>
    root?.render(
      onRender === undefined
        ? room
        : createElement(Profiler, { id: "room", onRender: (_, phase) => onRender(phase) }, room),
    ),
  );
  if (settled) await settle();
  return transport;
}

export async function settle() {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

export const rail = () => host?.querySelector<HTMLElement>("nav.room-rail") ?? null;
export const needs = () => rail()?.querySelector('[data-testid="rail-needs-you"]') ?? null;
export const namesIn = (el: Element | null | undefined) =>
  [...(el?.querySelectorAll(".room-matter-name") ?? [])].map((n) => n.textContent);
export const rowOf = (name: string) =>
  [...(rail()?.querySelectorAll<HTMLElement>('[data-testid="matter-row"]') ?? [])].find(
    (r) => r.querySelector(".room-matter-name")?.textContent === name,
  );
export const headings = () =>
  [...(rail()?.querySelectorAll(".room-when") ?? [])].map((h) => h.textContent);

export async function unmount() {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
}

export const theHost = () => host;
