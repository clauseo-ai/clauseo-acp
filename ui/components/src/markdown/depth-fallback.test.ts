import type { Nodes as Hast } from "hast";
import { expect, test, vi } from "vitest";
import { createAnswer } from "./answer.ts";
import { MAX_TREE_DEPTH } from "./nesting.ts";

// With nesting.ts's input limits off, the depth fallback after parsing still holds, for a block and for
// a note (round 1 of PR #115's review: the note view skipped it, so a note rendered 1,000 nested lists).
vi.mock("./nesting.ts", async (original) => ({
  ...(await original<typeof import("./nesting.ts")>()),
  capNesting: (text: string) => text,
}));

function depthOf(root: Hast): number {
  let deepest = 0;
  const stack: [Hast, number][] = [[root, 0]];
  for (let item = stack.pop(); item; item = stack.pop()) {
    const [node, depth] = item;
    deepest = Math.max(deepest, depth);
    if ("children" in node) for (const child of node.children) stack.push([child as Hast, depth + 1]);
  }
  return deepest;
}

function textOf(node: Hast): string {
  if (node.type === "text") return node.value;
  return "children" in node ? node.children.map((child) => textOf(child as Hast)).join("") : "";
}

test.each([true, false])(
  "a block and a note nested past the depth limit show as text (streaming %s)",
  (streaming) => {
    // 100 levels: past the limit, yet shallow enough to walk without overflowing the stack (1,000 overflow
    // in some engines and not in others, which the catch-all fallback covers).
    const deep = `${"- ".repeat(100)}x`;
    const views = createAnswer("t").update(`${deep}\n\nx[^a]\n\n[^a]: ${deep}`, streaming);
    const block = views[0]!;
    const note = views.at(-1)?.items?.[0];
    expect(note?.note).toBe("a");
    for (const view of [block, note!]) {
      expect(depthOf(view.hast)).toBeLessThan(MAX_TREE_DEPTH);
      expect(textOf(view.hast)).toContain("- - - x");
    }
  },
);
