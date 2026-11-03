// @vitest-environment happy-dom
// The clock time on an opened step group's rows (#263): the host's finished_at, else its started_at, in the thread's
// clock words (clock.ts stamp: the weekday before it on another day); none while a step runs; never on the folded
// line. The suite pins TZ=UTC, so every time here is built from a local midnight, never a literal UTC date.
import { afterEach, expect, test } from "vitest";
import { agentText, reset, toolCall, userMessage } from "../../testing/stream-script.ts";
import type { ToolCallEntry, ToolCallStatus, ViewEntry } from "../../transport/protocol.ts";
import { stamp } from "./clock.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

/** Today's local midnight, `days` away, then `h`:`m` on that day's clock. */
const local = (days: number, hh: number, mm: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days, hh, mm).getTime();
};

const read = (
  order: number,
  n: number,
  status: ToolCallStatus,
  over: Partial<ToolCallEntry>,
): ViewEntry => ({
  tool_call: {
    ...toolCall(order, n, status).tool_call!,
    kind: "read",
    title: `Tool ${n}`,
    locations: [{ inside: { path: `papers/Paper ${n}.pdf` } }],
    ...over,
  },
});

const ASKED = userMessage(5, 1, "What does the lease say about notice?");

/** The room with these calls before an answer, its group opened; the rows' time elements in order (null for none). */
async function opened(calls: ViewEntry[]) {
  h = await mountRoom();
  h.deliver([reset([ASKED, ...calls, agentText(20, 2, "Clause nine needs notice.")])]);
  const group = h.all("step-group")[0]!;
  const line = group.querySelector<HTMLButtonElement>("button")!;
  // folded, the line carries no time of its own
  expect(line.querySelector("time")).toBeNull();
  await h.click(line);
  return h.all("tool-call").map((r) => r.querySelector<HTMLTimeElement>("time.step-row-time"));
}

test("a finished row shows its finished time, at its end, from the thread's clock words", async () => {
  const started = local(0, 16, 30);
  const finished = local(0, 16, 32);
  const [time] = await opened([
    read(7, 201, "completed", { started_at: started, finished_at: finished }),
  ]);
  expect(time?.textContent).toBe("4:32 pm");
  expect(time?.textContent).toBe(stamp("en", finished, Date.now()));
  expect(time?.dateTime).toBe(new Date(finished).toISOString());
  expect(time?.parentElement?.lastElementChild).toBe(time);
});

test("a finished row from a host with no finished_at falls back to its start", async () => {
  const started = local(0, 9, 5);
  const [time] = await opened([read(7, 201, "completed", { started_at: started })]);
  expect(time?.textContent).toBe("9:05 am");
});

test("a running row shows no time; a waiting one neither", async () => {
  const [done, running, waiting] = await opened([
    read(7, 201, "completed", { started_at: local(0, 16, 30), finished_at: local(0, 16, 31) }),
    read(8, 202, "in_progress", { started_at: local(0, 16, 31) }),
    read(9, 203, "pending", { started_at: local(0, 16, 31) }),
  ]);
  expect(done?.textContent).toBe("4:31 pm");
  expect(running).toBeNull();
  expect(waiting).toBeNull();
});

test("a failed or stopped row shows its finished time", async () => {
  const [failed, stopped] = await opened([
    read(7, 201, "failed", { started_at: local(0, 11, 0), finished_at: local(0, 11, 2) }),
    read(8, 202, "cancelled", { started_at: local(0, 11, 3), finished_at: local(0, 11, 7) }),
  ]);
  expect(failed?.textContent).toBe("11:02 am");
  expect(stopped?.textContent).toBe("11:07 am");
});

test("a row from another local day takes the weekday before its time, as the thread's other times do", async () => {
  const yesterday = local(-1, 23, 58);
  const [time] = await opened([
    read(7, 201, "completed", { started_at: local(-1, 23, 57), finished_at: yesterday }),
  ]);
  const weekday = new Intl.DateTimeFormat("en-GB", { weekday: "long" }).format(new Date(yesterday));
  expect(time?.textContent).toBe(`${weekday} 11:58 pm`);
  expect(time?.textContent).toBe(stamp("en", yesterday, Date.now()));
});
