// @vitest-environment happy-dom A helper's asks name it (helper-asker.tsx, from C2's #1310: a
// permission request's or a question's `helper`), and the lead's say nothing more; and ChatGPT's
// helper as #1310 sends it: a name it chose, no task and no launching step, model gpt-6-luna, a
// background helper still working after the lead's turn ended, its report its final answer.
import { afterEach, describe, expect, test } from "vitest";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { ONE_QUESTION, askEntry } from "../elicitation-fixtures.ts";
import { permission, step, upsert, uuid } from "../../testing/stream-script.ts";
import { helper, helperId, helperStep, helperText } from "../../testing/helper-script.ts";
import { cards, cleanup, h, room, text } from "./helper-card-fixtures.ts";
import { askedBy } from "./helper-entries.ts";

afterEach(cleanup);

const chatgpt = (over: Parameters<typeof helper>[2] = {}) =>
  helper(10, 1, { name: "Read documents", model: "gpt-6-luna", ...over });

describe("who asks", () => {
  test("a helper's permission request names the helper above it", async () => {
    await room(
      [
        chatgpt(),
        helperStep(11, 60, 1, "pending", "pdftotext deed.pdf", "execute"),
        askedBy(permission(12, 60), helperId(1)),
      ],
      "running",
      "codex-acp",
    );
    const asker = h!.one("helper-asker");
    expect(text(asker)).toBe(steps("en", "{helper} asks:", { helper: "Read documents" }));
    expect(asker?.dataset["helper"]).toBe(helperId(1));
    // above the prompt, in the permission's row
    expect(
      asker?.closest(`[data-entry-id]`)?.querySelector("[data-testid^='permission']"),
    ).not.toBeNull();
  });

  test("a helper's question names the helper above it", async () => {
    await room(
      [chatgpt(), askedBy(askEntry(12, uuid(1, 9), ONE_QUESTION), helperId(1))],
      "running",
      "codex-acp",
    );
    const asker = h!.one("helper-asker");
    expect(text(asker)).toBe(steps("en", "{helper} asks:", { helper: "Read documents" }));
    expect(asker?.closest('[data-testid="elicitation"]')).not.toBeNull();
  });

  test("the lead's permission request and question say nothing more", async () => {
    await room([
      chatgpt(),
      helperStep(11, 61, null, "pending", "rm notes.txt", "execute"),
      permission(12, 61),
      askEntry(13, uuid(2, 9), ONE_QUESTION),
    ]);
    expect(h!.all("helper-asker")).toHaveLength(0);
    expect(h!.all("elicitation")).toHaveLength(1);
  });

  test("a helper the window has no entry for: a helper asks", async () => {
    await room([askedBy(permission(12, 62, "pending", null), helperId(9))], "running");
    expect(text(h!.one("helper-asker"))).toBe(steps("en", "A helper asks:"));
  });

  test("an unnamed helper is named as its card names it", async () => {
    await room(
      [
        helper(10, 1, { name: "default" }),
        askedBy(permission(12, 63, "pending", null), helperId(1)),
      ],
      "running",
    );
    expect(text(h!.one("helper-asker"))).toBe(
      steps("en", "{helper} asks:", { helper: steps("en", "Helper {n}", { n: 1 }) }),
    );
  });
});

describe("ChatGPT's helper, as #1310 sends it", () => {
  test("its own name and model, no task; working, then in the background, then its final answer", async () => {
    await room(
      [
        chatgpt(),
        helperText(11, 1, 1, "I'll read the lease first."),
        helperStep(12, 60, 1, "completed"),
      ],
      "running",
      "codex-acp",
    );
    const card = cards()[0]!;
    expect(text(h!.one("helper-name"))).toBe("Read documents");
    expect(text(card)).toContain("6 Luna");
    expect(h!.one("helper-task")).toBeNull();
    expect(text(h!.one("helper-state"))).toMatch(/^Working for /);
    // the lead's turn ends by itself: a background helper stays open (the host ends it later)
    h!.deliver([upsert(step(20, { ts: 10_000 + 60_000 }))]);
    await h!.settle();
    expect(cards()[0]!.dataset["state"]).toBe("working");
    expect(text(h!.one("helper-state"))).toBe(steps("en", "Working in the background"));
    // its end, with its final answer as the report; its words stay its words, not the report
    h!.deliver([
      upsert(
        chatgpt({
          outcome: "done",
          finished_at: 10_000 + 3 * 60_000,
          summary: { inline: "Lease start: 5 May 2026. Rent: Rs 2,40,000 a month." },
        }),
      ),
    ]);
    await h!.settle();
    expect(cards()[0]!.dataset["state"]).toBe("done");
    expect(text(h!.one("helper-state"))).toBe("Took 3 min");
    const found = text(h!.one("helper-found"));
    expect(found).toContain("Lease start: 5 May 2026.");
    expect(found).not.toContain("I'll read the lease first.");
  });
});
