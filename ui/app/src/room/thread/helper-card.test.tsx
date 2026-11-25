// @vitest-environment happy-dom A helper's card in the answer (H1, helper-card.tsx), from
// stream-script entries shaped as C2's CONTRACT.md §6: Claude's helper at its launching step with
// its label and task, ChatGPT's at its own place with neither; its steps out of the lead's groups
// and under its card; a step of a helper not loaded, flat; two and six at once; nesting; the turn
// limit. The working line, the words and how a helper ended: helper-card-words.test.tsx.
import { act } from "react";
import { afterEach, describe, expect, test } from "vitest";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { agentText, step, upsert } from "../../testing/stream-script.ts";
import {
  agentCall,
  callId,
  helper,
  helperId,
  helperStep,
  helperText,
  LIMIT_SUMMARY,
} from "../../testing/helper-script.ts";
import { entryIdOf } from "../../store/view.ts";
import { cards, cleanup, h, room, TASK, text } from "./helper-card-fixtures.ts";
import { leadRows } from "./helper-view.ts";
import { partsLine } from "./step-summary.ts";
import type { HelperEntry } from "./helper-entries.ts";

afterEach(cleanup);

describe("Claude's helper, with its launching step and task", () => {
  const claude = (over: Partial<HelperEntry> = {}, labelled = true) => [
    agentCall(10, 50, "in_progress", "Trace the title"),
    helper(11, 1, {
      call: callId(50),
      ...(labelled ? { label: "Trace the title to Plot 14" } : {}),
      task: { inline: TASK },
      model: "claude-haiku-5-5",
      ...over,
    }),
    helperStep(12, 60, 1, "completed"),
    helperText(13, 1, 1, "The 2004 deed names the vendor. "),
    helperText(14, 2, 1, "Checking the 1998 deed next."),
    helperStep(15, 61, 1, "in_progress"),
  ];

  test("working: its card stands in the Agent step's place with name, model, label, the step in hand and the time", async () => {
    await room(claude(), "running");
    expect(cards()).toHaveLength(1);
    const card = cards()[0]!;
    expect(card.dataset["state"]).toBe("working");
    expect(text(h!.one("helper-name"))).toBe("Paper reader");
    expect(text(card)).toContain("Haiku 5.5");
    expect(text(h!.one("helper-task"))).toBe("Trace the title to Plot 14");
    expect(text(h!.one("helper-now"))).toContain("Reading Sale deed 61.pdf.");
    expect(text(h!.one("helper-state"))).toMatch(/^Working for /);
    // the Agent step is not drawn, and the helper's steps join no lead group
    expect(h!.all("step-group")).toHaveLength(0);
    expect(h!.host.querySelector(`[data-tool-call-id="${callId(50)}"]`)).toBeNull();
    expect(text(h!.host)).not.toContain("Asked a helper");
  });

  test("its steps and words open under the card, the words as one paragraph", async () => {
    await room(claude(), "running");
    expect(h!.one("helper-rows")).toBeNull();
    await h!.click(cards()[0]!.querySelector("button"));
    const rows = h!.one("helper-rows")!;
    // two steps and one paragraph of its words
    expect(rows.querySelectorAll(".step-row")).toHaveLength(3);
    expect(text(rows)).toContain("The 2004 deed names the vendor. Checking the 1998 deed next.");
    expect(rows.querySelectorAll(".helper-words")).toHaveLength(1);
    expect(cards()[0]!.querySelector("button")!.getAttribute("aria-expanded")).toBe("true");
  });

  test("with no label, the task cut short stands for it", async () => {
    await room(claude({}, false));
    expect(text(h!.one("helper-task"))).toBe(TASK);
  });

  test("done: the start of its report, what it did and how long it took", async () => {
    await room(
      claude({
        outcome: "done",
        finished_at: 11_000 + 4 * 60_000 + 20_000,
        summary: {
          inline: "The chain breaks in 1998: the vendor had no title.\n\nDetails follow.",
        },
      }).map((e) =>
        e.tool_call?.status === "in_progress" ? helperStep(15, 61, 1, "completed") : e,
      ),
    );
    const card = cards()[0]!;
    expect(card.dataset["state"]).toBe("done");
    expect(text(h!.one("helper-state"))).toBe("Took 4 min");
    expect(text(h!.one("helper-found"))).toContain(
      "The chain breaks in 1998: the vendor had no title.",
    );
    expect(text(h!.one("helper-found"))).not.toContain("Details follow");
    expect(text(h!.one("helper-found"))).toContain(
      partsLine("en", [steps("en", "read {n} papers", { n: 2 })]),
    );
  });

  test("failed with no report: it ended without reporting back to Claude, and what it did before", async () => {
    await room(claude({ outcome: "failed", finished_at: 40_000 }));
    expect(cards()[0]!.dataset["state"]).toBe("failed");
    expect(text(h!.one("helper-state"))).toBe("Failed after less than a minute");
    const found = text(h!.one("helper-found"));
    expect(found).toContain("It ended without reporting back to Claude.");
    expect(found).toContain("Before that: ");
  });

  test("stopped: how long it ran, and what it did by then", async () => {
    await room(claude({ outcome: "stopped", finished_at: 11_000 + 2 * 60_000 }));
    expect(cards()[0]!.dataset["state"]).toBe("stopped");
    expect(text(h!.one("helper-state"))).toBe("Stopped after 2 min");
    expect(text(h!.one("helper-found"))).toContain("By then: ");
  });

  test("an outcome from a newer Clauseo reads as stopped", async () => {
    await room(claude({ outcome: "unknown", finished_at: 20_000 }));
    expect(cards()[0]!.dataset["state"]).toBe("stopped");
  });
});

describe("ChatGPT's helper: no launching step, no task", () => {
  test("its card stands at its own place, with its name, and reads complete", async () => {
    await room([
      agentText(8, 1, "I'll have a helper read the documents."),
      helper(10, 1, {
        name: "Read documents",
        model: "gpt-6.2-sol",
        outcome: "done",
        finished_at: 70_000,
        summary: { inline: "Nine papers; the 1998 deed is the weak link." },
      }),
      helperStep(11, 60, 1, "completed", "cat '/matter/Sale deed 2004.pdf'", "execute"),
    ]);
    expect(cards()).toHaveLength(1);
    expect(text(h!.one("helper-name"))).toBe("Read documents");
    expect(text(cards()[0])).toContain("6.2 Sol");
    expect(h!.one("helper-task")).toBeNull();
    expect(text(h!.one("helper-found"))).toContain("Nine papers; the 1998 deed is the weak link.");
    expect(text(h!.one("helper-state"))).toBe("Took 1 min");
  });
});

describe("an unloaded helper's step", () => {
  test("shows flat in the lead's group, and its words flat, never dropped", async () => {
    await room([
      helperStep(10, 60, 9, "completed"),
      helperText(11, 1, 9, "Words of a helper on an older page."),
      agentText(12, 1, "Done."),
      step(20),
    ]);
    expect(cards()).toHaveLength(0);
    expect(h!.all("step-group")).toHaveLength(1);
    expect(text(h!.one("helper-text-flat"))).toBe("Words of a helper on an older page.");
  });

  test("leadRows keeps a step whose helper is unknown and drops one whose helper is known", () => {
    const entries = [
      helper(10, 1),
      helperStep(11, 60, 1, "completed"),
      helperStep(12, 61, 2, "completed"),
      helperStep(13, 62, null, "completed"),
    ];
    const map = new Map(entries.map((e) => [entryIdOf(e), e]));
    const ids = [...map.keys()];
    expect(leadRows(ids, (id) => map.get(id))).toEqual([ids[0], ids[2], ids[3]]);
  });

  test("a card fills in when its helper's entry arrives after its steps", async () => {
    await room([helperStep(12, 60, 1, "completed")], "running");
    expect(cards()).toHaveLength(0);
    h!.deliver([upsert(helper(10, 1, { name: "researcher" }))]);
    await h!.settle();
    expect(cards()).toHaveLength(1);
    expect(text(h!.one("helper-name"))).toBe("Researcher");
  });
});

describe("several at once", () => {
  const many = (n: number) =>
    Array.from({ length: n }, (_, i) => [
      agentCall(10 + i * 3, 50 + i, "in_progress", `Read deed ${i + 1}`),
      helper(11 + i * 3, i + 1, {
        call: callId(50 + i),
        label: `Read deed ${i + 1}`,
        model: "haiku",
      }),
      helperStep(12 + i * 3, 60 + i, i + 1, "in_progress"),
    ]).flat();

  test("two: two cards, each with its own live line; the working line waits on both", async () => {
    await room(many(2), "running");
    expect(cards()).toHaveLength(2);
    expect(cards().map((c) => c.dataset["helper"])).toEqual([helperId(1), helperId(2)]);
    expect(h!.all("helper-now").map(text)).toEqual([
      expect.stringContaining("Sale deed 60.pdf"),
      expect.stringContaining("Sale deed 61.pdf"),
    ]);
  });

  test("six render, each in its launching step's place", async () => {
    await room(many(6), "running");
    expect(cards()).toHaveLength(6);
    expect(h!.all("helper-task").map(text)).toEqual(
      [1, 2, 3, 4, 5, 6].map((n) => `Read deed ${n}`),
    );
    expect(h!.all("step-group")).toHaveLength(0);
  });
});

test("the card shows the model menu's own name for the id when the conversation lists it", async () => {
  await room([helper(10, 1, { model: "claude-haiku-5-5" })], "running", "claude-acp", (head) => {
    head.header!.config_options = [
      {
        id: "model",
        name: "Model",
        category: "model",
        kind: {
          select: {
            current_value: "claude-opus-5-5",
            options: [
              { value: "claude-opus-5-5", name: "Opus 5.5" },
              { value: "claude-haiku-5-5", name: "Haiku 5.5 (fast)" },
            ],
          },
        },
      },
    ] as never;
  });
  expect(text(cards()[0])).toContain("Haiku 5.5 (fast)");
  await act(async () => {});
});

describe("a helper a helper started (ChatGPT)", () => {
  test("has its own small card inside its parent's, with its own name, state and steps; deeper folds into it", async () => {
    await room(
      [
        helper(10, 1, { name: "Read documents" }),
        helperStep(11, 60, 1, "completed"),
        helper(12, 2, { name: "Find the clause", parent: helperId(1) }),
        helperStep(13, 61, 2, "completed"),
        helper(14, 3, { name: "Deeper", parent: helperId(2) }),
        helperStep(15, 62, 3, "in_progress"),
        helper(16, 2, {
          name: "Find the clause",
          parent: helperId(1),
          outcome: "done",
          finished_at: 70_000,
          summary: { inline: "Clause 7 of Sale_deed_1998.pdf." },
        }),
      ],
      "running",
      "codex-acp",
    );
    expect(cards()).toHaveLength(1);
    const kids = h!.all("helper-child");
    expect(kids).toHaveLength(1);
    expect(cards()[0]!.contains(kids[0]!)).toBe(true);
    expect(kids[0]!.dataset["state"]).toBe("done");
    expect(text(kids[0]!.querySelector('[data-testid="helper-name"]'))).toBe("Find the clause");
    expect(text(kids[0]!)).toContain("Clause 7 of Sale_deed_1998.pdf.");
    // the parent lists its own step only; the child its own and the deeper one's
    await h!.click(cards()[0]!.querySelector("button"));
    const own = h!.one("helper-rows")!;
    expect(own.querySelectorAll(".step-row")).toHaveLength(1);
    expect(text(own)).toContain("Sale deed 60.pdf");
    await h!.click(kids[0]!.querySelector("button"));
    const folded = kids[0]!.querySelector('[data-testid="helper-rows"]')!;
    expect(folded.querySelectorAll(".step-row")).toHaveLength(2);
    expect(text(folded)).toContain("Sale deed 62.pdf");
  });
});

describe("a helper cut at its turn limit (C2's recorded run L4)", () => {
  const limited = (over: Partial<HelperEntry> = {}) =>
    helper(11, 1, {
      call: callId(50),
      label: "Digest four matter files",
      outcome: "done",
      finished_at: 11_000 + 2 * 60_000,
      summary: { inline: LIMIT_SUMMARY },
      ...over,
    });
  const before = [agentCall(10, 50, "completed", "Digest four matter files"), limited()];

  test("reads done, the limit said plainly, never the adapter's bracket or its words for the lead", async () => {
    await room([...before, helperStep(12, 60, 1, "failed")]);
    const card = cards()[0]!;
    expect(card.dataset["state"]).toBe("done");
    expect(text(h!.one("helper-state"))).toBe("Took 2 min");
    expect(text(h!.one("helper-found"))).toContain(
      steps("en", "It stopped at its turn limit before reporting back to {agent}.", {
        agent: "Claude",
      }),
    );
    expect(text(card)).not.toContain("[Agent stopped");
    expect(text(card)).not.toContain("SendMessage");
  });

  test("what it wrote after the note still shows, after the limit", async () => {
    await room([
      agentCall(10, 50, "completed", "Digest"),
      limited({
        summary: { inline: `${LIMIT_SUMMARY.split("\n")[0]}\n\nThe 1998 deed is missing.` },
      }),
    ]);
    const found = text(h!.one("helper-found"));
    expect(found).toContain("turn limit");
    expect(found).toContain("The 1998 deed is missing.");
  });

  test("resumed by the lead's message, its later steps land under the same card", async () => {
    await room(
      [
        ...before,
        helperStep(12, 60, 1, "failed"),
        // the lead's SendMessage, then the helper's steps, stamped with the same helper
        helperStep(13, 70, null, "completed", "SendMessage", "other"),
        helperStep(14, 61, 1, "failed"),
        helperStep(15, 62, 1, "in_progress"),
      ],
      "running",
    );
    expect(cards()).toHaveLength(1);
    expect(cards()[0]!.dataset["state"]).toBe("done");
    await h!.click(cards()[0]!.querySelector("button"));
    expect(h!.one("helper-rows")!.querySelectorAll(".step-row")).toHaveLength(3);
    for (const n of [60, 61, 62]) {
      const row = h!.host.querySelector(`[data-tool-call-id="${callId(n)}"]`);
      expect(row?.closest('[data-testid="helper-card"]')).not.toBeNull();
    }
  });
});

test("two helpers at once: their steps interleave, each lands on its own card", async () => {
  await room(
    [
      agentCall(10, 50, "completed", "A"),
      helper(11, 1, { call: callId(50) }),
      agentCall(12, 51, "completed", "B"),
      helper(13, 2, { call: callId(51) }),
      helperStep(14, 60, 1, "completed"),
      helperStep(15, 61, 2, "completed"),
      helperStep(16, 62, 1, "in_progress"),
      helperStep(17, 63, 2, "in_progress"),
    ],
    "running",
  );
  expect(h!.all("helper-now").map(text)).toEqual([
    expect.stringContaining("Sale deed 62.pdf"),
    expect.stringContaining("Sale deed 63.pdf"),
  ]);
  for (const c of cards()) await h!.click(c.querySelector("button"));
  const rows = h!.all("helper-rows");
  const ids = (el: Element) =>
    [...el.querySelectorAll("[data-tool-call-id]")].map((r) => r.getAttribute("data-tool-call-id"));
  expect(ids(rows[0]!)).toEqual([callId(60), callId(62)]);
  expect(ids(rows[1]!)).toEqual([callId(61), callId(63)]);
});
