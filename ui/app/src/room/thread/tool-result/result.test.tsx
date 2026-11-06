// @vitest-environment happy-dom
// A row's result (U17) over FakeTransport: the press only on an eligible row, the rest byte-identical; the block's
// forms, its fold at 12 lines and its cap; an error turning the row to Failed with the folded line unchanged; every
// hostile output as literal text, no element, link or picture; a blob read once, on the press, its reading words only
// past 400 ms; the parser loaded on the first press and not before; and opening one row rendering that row alone.
import axe from "axe-core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, expect, test, vi } from "vitest";
import { StepRow } from "@clauseo/components/clauseo/step-group.tsx";
import { ToolKindIcon } from "@clauseo/components/clauseo/tool-kind-icons.tsx";
import { settled } from "@clauseo/components/lib/late.ts";
import { agentText, reset, upsert, userMessage } from "../../../testing/stream-script.ts";
import { FakeTransport, type FakeHost } from "../../../transport/fake-transport.ts";
import type { HostQuery, QueryResult, ToolCallEntry } from "../../../transport/protocol.ts";
import { mountRoom, type Harness } from "../harness.ts";
import { SourcesTransport } from "../sources.ts";
import { CallRow } from "../step-group-parts.tsx";
import { doneOf, hoverOf, markOf } from "../steps.ts";
import { forgetBlobsForTests } from "./blob.ts";
import * as F from "./fixtures.ts";

// the parser, watched: imported (its module evaluated) and called only once a result is shown
const parser = vi.hoisted(() => ({ imported: false, calls: 0 }));
vi.mock("./parse.ts", async (original) => {
  parser.imported = true;
  const real = await original<typeof import("./parse.ts")>();
  return {
    ...real,
    readOutput: (raw: string) => {
      parser.calls += 1;
      return real.readOutput(raw);
    },
  };
});
// each row's renders, counted by its call: CallRow draws one StepRow, a new one only when it renders anew
const renders = vi.hoisted(() => new Map<string, number>());
vi.mock("@clauseo/components/clauseo/step-group.tsx", async (original) => {
  const real = await original<typeof import("@clauseo/components/clauseo/step-group.tsx")>();
  return {
    ...real,
    StepRow: (props: Parameters<typeof real.StepRow>[0]) => {
      renders.set(props.toolCall, (renders.get(props.toolCall) ?? 0) + 1);
      return real.StepRow(props);
    },
  };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let el: HTMLElement | null = null;
let h: Harness | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  el?.remove();
  root = el = null;
  await h?.unmount();
  h = null;
  forgetBlobsForTests();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const settle = () =>
  act(async () => {
    await settled();
    for (let i = 0; i < 3; i += 1) await new Promise((r) => setTimeout(r, 0));
  });

/** The rows of an open group for `calls`, over a FakeTransport answering read_blob from `blobs` (a text, or a refusal). */
async function mount(calls: ToolCallEntry[], blobs: Record<string, string | Error> = {}) {
  const host: FakeHost = {
    query: (q: HostQuery): QueryResult => {
      const read = typeof q === "object" ? q.read_blob : undefined;
      const got = read === undefined ? undefined : blobs[read.hash];
      if (got === undefined) throw new Error(`not scripted: ${JSON.stringify(q)}`);
      if (got instanceof Error) throw { code: "store.io", message: got.message, retryable: false };
      return { blob: { offset: 0, end: got.length, text: got } };
    },
  };
  const transport = new FakeTransport(host);
  const draw = (list: ToolCallEntry[]) => (
    <SourcesTransport value={transport}>
      <main>
        <ol className="step-group-rows">
          {list.map((c) => (
            <CallRow key={c.tool_call} id={`e-${c.tool_call}`} call={c} lang="en" />
          ))}
        </ol>
      </main>
    </SourcesTransport>
  );
  el = document.createElement("div");
  document.body.append(el);
  root = createRoot(el);
  await act(async () => root!.render(draw(calls)));
  const rows = () => [...el!.querySelectorAll<HTMLLIElement>("li.step-row")];
  const press = (i: number) => rows()[i]!.querySelector<HTMLButtonElement>(".step-row-press")!;
  return {
    transport,
    rows,
    press,
    block: (i: number) => rows()[i]!.querySelector<HTMLElement>('[data-testid="tool-result"]'),
    text: (i: number) =>
      rows()[i]!.querySelector<HTMLElement>('[data-testid="tool-result-text"]')?.textContent,
    click: async (b: Element | null | undefined) => {
      await act(async () => (b as HTMLElement).click());
      await settle();
    },
    redraw: (list: ToolCallEntry[]) => act(async () => root!.render(draw(list))),
    reads: () => transport.calls.filter((c) => c.call === "query"),
  };
}

test("the parser is neither imported nor run until a result is shown", async () => {
  const m = await mount([F.called(F.JSON_RESULT), F.called(F.MCP_TEXT)]);
  await settle();
  expect(m.press(0).textContent).toBe("Show the result");
  expect(parser.imported).toBe(false);
  expect(parser.calls).toBe(0);
  await m.click(m.press(0));
  expect(parser.imported).toBe(true);
  expect(parser.calls).toBe(1);
  expect(m.block(0)).not.toBeNull();
});

test("the press shows only on an eligible row; any other row renders as it did before U17", async () => {
  const shown = [
    F.called(F.JSON_RESULT),
    F.called(F.OPAQUE_ERROR, { status: "failed" }),
    F.called(F.MCP_TEXT, { status: "cancelled" }),
    F.called({ blob: F.BLOB }, { kind: "fetch" }),
  ];
  const plain = [
    F.called(undefined),
    F.called(F.JSON_RESULT, { content: [{ content: { text: { text: "Read." } } }] as never }),
    F.called(F.JSON_RESULT, { status: "pending" }),
    F.called(F.JSON_RESULT, { status: "in_progress" }),
    F.called(F.JSON_RESULT, { status: "unknown" }),
    F.called(F.JSON_RESULT, { kind: "think" }),
    F.called(F.JSON_RESULT, { kind: "switch_mode" }),
  ];
  const m = await mount([...shown, ...plain]);
  m.rows().forEach((row, i) => {
    const press = row.querySelector(".step-row-press");
    if (i < shown.length) {
      expect(press?.getAttribute("aria-expanded")).toBe("false");
      expect(press?.getAttribute("aria-controls")).toMatch(/.+/);
      expect(row.dataset["result"]).toBe("closed");
    } else expect(press).toBeNull();
  });
  // byte for byte the row StepRow drew before U17, no slot and no attribute added
  for (const call of plain) {
    const now = renderToStaticMarkup(<CallRow id="e" call={call} lang="en" />);
    const before = renderToStaticMarkup(
      <StepRow
        id="e"
        toolCall={call.tool_call}
        status={call.status}
        kind={call.kind}
        text={doneOf("en", call)}
        hover={hoverOf(call)}
        mark={markOf("en", call)}
        background={false}
        time={null}
      />,
    );
    // the time column is the clock's, the same in both; compare the rest
    expect(now.replace(/<time[^>]*>.*?<\/time>/, "")).toBe(before);
  }
});

test("each form: MCP text, pretty JSON in the code face, a plain string, raw text, nothing came back, blocks counted", async () => {
  const m = await mount([
    F.called(F.MCP_TEXT),
    F.called(F.JSON_RESULT),
    F.called(F.QUOTED),
    F.called(F.NOT_JSON),
    F.called(F.NULL),
    F.called(F.NO_BLOCKS),
    F.called(F.MCP_MIXED),
    F.called(F.QUOTED_OBJECT),
  ]);
  for (let i = 0; i < 8; i += 1) await m.click(m.press(i));
  expect(m.press(0).textContent).toBe("Hide the result");
  expect(m.press(0).getAttribute("aria-expanded")).toBe("true");
  expect(m.rows()[0]!.dataset["result"]).toBe("open");
  expect(m.block(0)!.id).toBe(m.press(0).getAttribute("aria-controls"));
  expect(m.text(0)).toBe("3 judgments found.");
  expect(m.block(0)!.dataset["face"]).toBe("words");
  expect(m.block(1)!.dataset["face"]).toBe("code");
  expect(m.text(1)!.split("\n")[1]).toBe('  "court": "Supreme Court of India",');
  expect(m.text(2)).toBe("Done");
  expect(m.text(3)).toBe("exit 0: 14 files scanned");
  expect(m.block(3)!.dataset["face"]).toBe("code");
  expect(m.block(4)!.textContent).toBe("Nothing came back.");
  expect(m.block(5)!.textContent).toBe("Nothing came back.");
  expect(m.block(6)!.textContent).toBe(
    "The page has two figures.2 pictures aren't shown here.1 more item isn't shown here.",
  );
  expect(m.text(7)).toBe("ok");
  // the text sits in one element, pre-wrap by its class, dir="auto", inside <bdi>
  const t = m.rows()[1]!.querySelector('[data-testid="tool-result-text"]')!;
  expect(t.getAttribute("dir")).toBe("auto");
  expect(t.parentElement!.tagName).toBe("BDI");
  expect(t.className).toBe("step-row-result-text");
});

test("past 12 lines the block folds with Show all; pressed, every line shows; hidden and shown, it folds again; past the cap, a line says so", async () => {
  const m = await mount([F.called(F.LONG_RESULT), F.called(F.HUGE_RESULT)]);
  await m.click(m.press(0));
  expect(m.text(0)!.split("\n")).toHaveLength(12);
  const all = m.block(0)!.querySelector<HTMLButtonElement>(".step-row-press")!;
  expect(all.textContent).toBe("Show all");
  await m.click(all);
  expect(m.text(0)!.split("\n")).toHaveLength(22);
  expect(m.block(0)!.querySelector(".step-row-press")).toBeNull();
  expect(m.block(0)!.textContent).not.toContain("The rest of this result");
  await m.click(m.press(0));
  expect(m.block(0)).toBeNull();
  await m.click(m.press(0));
  expect(m.text(0)!.split("\n")).toHaveLength(12);
  await m.click(m.press(1));
  expect(m.block(1)!.textContent).not.toContain("The rest of this result");
  await m.click(m.block(1)!.querySelector(".step-row-press"));
  expect(m.text(1)!.split("\n")).toHaveLength(2_000);
  expect(m.block(1)!.lastElementChild!.textContent).toBe(
    "The rest of this result isn't shown here.",
  );
});

test("an error shows its text and turns the row to Failed with the break, whatever its status", async () => {
  const m = await mount([
    F.called(F.OPAQUE_ERROR),
    F.called(F.OPAQUE_ERROR, { status: "failed" }),
    F.called(F.MCP_ERROR, { status: "cancelled" }),
  ]);
  const breakIcon = renderToStaticMarkup(
    <ToolKindIcon kind="other" failed size={14} className="step-icon" />,
  );
  const mark = (i: number) => m.rows()[i]!.querySelector(".step-row-mark")?.textContent ?? null;
  expect(mark(0)).toBeNull();
  expect(mark(2)).toBe("Stopped");
  for (const i of [0, 1, 2]) await m.click(m.press(i));
  expect(m.text(0)).toBe("connector: out of credit");
  expect(m.text(2)).toBe("Indian Kanoon refused the search: the key was not accepted.");
  for (const i of [0, 1, 2]) {
    expect(mark(i)).toBe("Failed");
    expect(m.block(i)!.dataset["error"]).toBe("true");
    expect(m.rows()[i]!.querySelector("svg")!.outerHTML).toBe(breakIcon);
  }
  expect(m.rows()[0]!.dataset["failed"]).toBe("true");
  // its status stays the host's
  expect(m.rows()[0]!.dataset["status"]).toBe("completed");
  // hidden, the row keeps what it read
  await m.click(m.press(0));
  expect(mark(0)).toBe("Failed");
});

test("every hostile output is literal text: no element but the block's own, no href, no src, and its URL opens nothing", async () => {
  const keys = Object.keys(F.HOSTILE);
  const m = await mount(keys.map((k) => F.called(F.HOSTILE[k])));
  for (let i = 0; i < keys.length; i += 1) await m.click(m.press(i));
  for (let i = 0; i < keys.length; i += 1) {
    const block = m.block(i)!;
    const text = block.querySelector('[data-testid="tool-result-text"]')!;
    expect(
      [...text.childNodes].every((n) => n.nodeType === Node.TEXT_NODE),
      keys[i],
    ).toBe(true);
    const tags = [...block.querySelectorAll("*")].map((e) => e.tagName.toLowerCase());
    expect(
      tags.every((t) => ["span", "bdi", "button"].includes(t)),
      keys[i],
    ).toBe(true);
    expect(block.querySelector("[href], [src], a, img, script")).toBeNull();
    const attrs = [...block.querySelectorAll("*")].flatMap((e) => e.getAttributeNames());
    expect(attrs.filter((a) => a === "href" || a === "src" || a.startsWith("on"))).toEqual([]);
  }
  expect(m.text(keys.indexOf("img"))).toBe("<img src=x onerror=alert(1)>");
  expect(m.text(keys.indexOf("script"))).toBe("<script>alert(1)</script>");
  expect(m.text(keys.indexOf("line"))).toHaveLength(10_000);
  const url = m.block(keys.indexOf("url"))!.querySelector("span")!;
  await m.click(url);
  expect(m.transport.calls.filter((c) => c.call === "open_external")).toEqual([]);
  const result = await axe.run(el!, { rules: { region: { enabled: false } } });
  expect(result.violations.map((v) => v.id)).toEqual([]);
});

test("a blob is read only on the press, once, its first 64 KB; never when past 64 KB; a refused read says too large and is logged", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const m = await mount(
    [
      F.called({ blob: F.BLOB }),
      F.called({ blob: F.BLOB_LARGE }),
      F.called({ blob: F.BLOB_REFUSED }),
    ],
    { [F.BLOB.hash]: F.BLOB_TEXT, [F.BLOB_REFUSED.hash]: new Error("missing") },
  );
  await settle();
  expect(m.reads()).toEqual([]);
  await m.click(m.press(0));
  expect(m.reads()).toEqual([
    { call: "query", query: { read_blob: { hash: F.BLOB.hash, offset: 0, len: 65_536 } } },
  ]);
  expect(m.text(0)).toBe(F.JUDGMENTS);
  await m.click(m.press(0));
  await m.click(m.press(0));
  expect(m.text(0)).toBe(F.JUDGMENTS);
  expect(m.reads()).toHaveLength(1);
  await m.click(m.press(1));
  expect(m.block(1)!.textContent).toBe("This result is too large to show here.");
  expect(m.reads()).toHaveLength(1);
  await m.click(m.press(2));
  expect(m.reads()).toHaveLength(2);
  expect(m.block(2)!.textContent).toBe("This result is too large to show here.");
  expect(warn.mock.calls.flat().join(" ")).toContain("read_blob failed");
  expect(m.block(2)!.textContent).not.toContain("store.io");
  // the preview is never shown
  expect(el!.textContent).not.toContain('{"result":{"cont');
});

test("the reading words show only once a blob's read has taken 400 ms, the press held while they do", async () => {
  // the block's chunk is in from the tests above; only the clock is faked
  let answer: (r: QueryResult) => void = () => {};
  const m = await mount([F.called({ blob: F.BLOB })]);
  m.transport.query = (q) => {
    m.transport.calls.push({ call: "query", query: q });
    return new Promise((r) => (answer = r));
  };
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  await act(async () => m.press(0).click());
  await act(() => vi.advanceTimersByTimeAsync(399));
  expect(m.press(0).textContent).toBe("Hide the result");
  expect(m.press(0).disabled).toBe(false);
  await act(() => vi.advanceTimersByTimeAsync(1));
  expect(m.press(0).textContent).toBe("Reading the result.");
  expect(m.press(0).disabled).toBe(true);
  await act(async () => answer({ blob: { offset: 0, end: 10, text: F.BLOB_TEXT } }));
  expect(m.press(0).textContent).toBe("Hide the result");
  expect(m.press(0).disabled).toBe(false);
  expect(m.text(0)).toBe(F.JUDGMENTS);
});

test("opening one row's result renders that row alone, and another call's update doesn't render an open one", async () => {
  const calls = [F.called(F.JSON_RESULT), F.called(F.MCP_TEXT), F.called(F.QUOTED)];
  const m = await mount(calls);
  const count = () => calls.map((c) => renders.get(c.tool_call) ?? 0);
  const before = count();
  await m.click(m.press(1));
  const opened = count();
  expect(opened[0]).toBe(before[0]);
  expect(opened[2]).toBe(before[2]);
  expect(opened[1]).toBeGreaterThan(before[1]!);
  await m.redraw([calls[0]!, calls[1]!, { ...calls[2]!, title: "Changed" }]);
  const after = count();
  expect(after[1]).toBe(opened[1]);
  expect(after[2]).toBe(opened[2]! + 1);
  expect(m.text(1)).toBe("3 judgments found.");
});

test("in the room: opening the group reads no blob; an error read turns its row to Failed and leaves the folded line as it was", async () => {
  const reads: HostQuery[] = [];
  h = await mountRoom({
    query: (q) => {
      reads.push(q);
      return { blob: { offset: 0, end: 1, text: F.OPAQUE_ERROR.inline! } };
    },
  });
  const blobbed = F.called({ blob: F.BLOB }, { order: 7 });
  h.deliver([
    reset([
      userMessage(5, 1, "Find judgments on section 138."),
      { tool_call: blobbed },
      { tool_call: F.called(F.MCP_TEXT, { order: 8 }) },
      agentText(9, 1, "Three judgments came back."),
    ]),
  ]);
  await h.settle();
  const group = h.one("step-group")!;
  const line = group.querySelector<HTMLButtonElement>("button")!;
  const folded = line.textContent;
  await h.click(line);
  expect(reads).toEqual([]);
  const row = h.all("tool-call")[0]!;
  await h.click(row.querySelector(".step-row-press"));
  await act(settled);
  await h.settle();
  expect(reads).toHaveLength(1);
  expect(row.querySelector(".step-row-mark")?.textContent).toBe("Failed");
  expect(line.textContent).toBe(folded);
  expect(group.dataset["trouble"]).toBeUndefined();
  // a patch to the call keeps the row's own state
  h.deliver([upsert({ tool_call: { ...blobbed, title: "Search" } })]);
  await h.settle();
  expect(h.all("tool-call")[0]!.querySelector(".step-row-mark")?.textContent).toBe("Failed");
});
