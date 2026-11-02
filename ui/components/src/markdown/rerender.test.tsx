// @vitest-environment happy-dom
// Finished blocks stay rendered while the tail streams: an answer streamed at 16-character deltas, with every
// block's renders counted through its error boundary (which renders exactly when the block does).
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";

const rendered: string[] = [];
vi.mock("./boundary.tsx", async (original) => {
  const { BlockBoundary } = await original<typeof import("./boundary.tsx")>();
  return {
    BlockBoundary(props: React.ComponentProps<typeof BlockBoundary>) {
      rendered.push(props.source);
      return <BlockBoundary {...props} />;
    },
  };
});
const { createAnswer } = await import("./answer.ts");
const { Markdown } = await import("./markdown.tsx");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const FENCE = "```";
// No reference links: a definition arriving later rightly re-renders the blocks that use it.
const SECTION = (n: number) =>
  [
    `## Section ${n}`,
    `Some prose for section ${n}, with *emphasis*, \`code\` and a [link](https://example.com/${n}).`,
    `1. The first point of section ${n} runs on for a while, so that a delta ends inside it\n   - with a nested item that also runs on for a few more words than it needs\n   - and another\n2. The second point, which is short`,
    `${FENCE}ts\nconst value${n} = ${n};\nfunction f${n}() {\n  return value${n} * 2;\n}\n${FENCE}`,
    `> A quote in section ${n}, long enough to stream over a few deltas.`,
    `| a | b |\n| - | - |\n| ${n} | ${n + 1} |`,
  ].join("\n\n");
const ANSWER = Array.from({ length: 6 }, (_, index) => SECTION(index + 1)).join("\n\n") + "\n";

test("a finished block renders once more as the next block starts, and never again", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const model = createAnswer("count");
  // How often each block rendered while it was not the streaming last block.
  const finishedRenders = new Map<string, number>();
  for (let length = 16; length < ANSWER.length + 16; length += 16) {
    const text = ANSWER.slice(0, length);
    const tail = model.update(text, true).at(-1)!.source;
    rendered.length = 0;
    act(() => root.render(<Markdown text={text} streaming />));
    for (const source of rendered)
      if (source !== tail) finishedRenders.set(source, (finishedRenders.get(source) ?? 0) + 1);
  }
  act(() => root.unmount());
  host.remove();
  expect(finishedRenders.size).toBeGreaterThan(30);
  // The one allowed re-render: the block just behind the tail, as the next block starts (it stops being the
  // streaming block, so its links go live). Any second render means a finished block re-rendered.
  expect([...finishedRenders].filter(([, times]) => times > 1)).toEqual([]);
  // A render per delta of the whole answer: well under a second alone, but no faster on a loaded machine.
}, 30_000);
