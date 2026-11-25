// @vitest-environment happy-dom A helper's card (helper-card.tsx), its working line and its words:
// the composer's line while the lead waits on its helpers, minutes, names and models, Markdown in
// a helper's words and report, how it ended (live, after the answer, in the background), the
// Yield-off order, and the opened words live. The card's place and contents: helper-card.test.tsx.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, test } from "vitest";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { agentText, header, step, upsert } from "../../testing/stream-script.ts";
import { agentCall, callId, helper, helperStep, helperText } from "../../testing/helper-script.ts";
import { entryIdOf, type StreamView } from "../../store/view.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import { cards, cleanup, h, room, text, viewOf } from "./helper-card-fixtures.ts";
import { minutesWords, modelWords, nameWords, stateWords } from "./helper-view.ts";
import { doingPick } from "./step-names.ts";
import type { HelperEntry } from "./helper-entries.ts";

afterEach(cleanup);

const cardCss = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "helper-card.css"),
  "utf8",
);

describe("the working line and the words", () => {
  const view = (status: "running" | "idle", entries: ViewEntry[]): StreamView =>
    ({
      header: header(status),
      ids: entries.map(entryIdOf),
      entries: new Map(entries.map((e) => [entryIdOf(e), e])),
    }) as unknown as StreamView;

  test("waiting for one helper, for two; the lead's own step in hand still says what it does", () => {
    const pick = doingPick("en");
    const one = [agentCall(10, 50, "in_progress", "Read"), helper(11, 1, { call: callId(50) })];
    expect(pick(view("running", [...one, helperStep(12, 60, 1, "in_progress")]))).toBe(
      stepLive("en", "Waiting for a helper to finish."),
    );
    const two = [
      ...one,
      agentCall(13, 51, "in_progress", "Read"),
      helper(14, 2, { call: callId(51) }),
    ];
    expect(pick(view("running", two))).toBe(
      stepLive("en", "Waiting for {n} helpers to finish.", { n: 2 }),
    );
    expect(pick(view("running", [...two, helperStep(15, 62, null, "in_progress")]))).toBe(
      "Reading Sale deed 62.pdf.",
    );
    const ended = [
      agentCall(10, 50, "completed", "Read"),
      helper(11, 1, { outcome: "done", finished_at: 1 }),
    ];
    expect(pick(view("running", ended))).toBeNull();
  });

  test("time in whole minutes, no seconds", () => {
    expect(minutesWords("en", 59_000)).toBe("less than a minute");
    expect(minutesWords("en", 60_000)).toBe("1 min");
    expect(minutesWords("en", 4 * 60_000 + 59_000)).toBe("4 min");
    const working = { order: 1, helper: "h", turn: "t", name: "x", started_at: 0 } as HelperEntry;
    expect(stateWords("en", working, 3 * 60_000 + 5_000, undefined)).toBe("Working for 3 min");
    expect(stateWords("en", working, 10_000, undefined)).toBe("Working for less than a minute");
    // still open after its turn ended: working in the background (the host ends it), no clock
    // under a finished answer
    expect(stateWords("en", working, 300 * 60_000, 2 * 60_000)).toBe("Working in the background");
  });

  test("names and models in the lawyer's words", () => {
    expect(nameWords("en", "paper-reader", 1)).toBe("Paper reader");
    expect(nameWords("en", "case-finder", 1)).toBe("Case finder");
    expect(nameWords("en", "default", 2)).toBe("Helper 2");
    expect(modelWords("claude-haiku-5-5")).toBe("Haiku 5.5");
    expect(modelWords("haiku")).toBe("Haiku");
    expect(modelWords("gpt-6.2-sol")).toBe("6.2 Sol");
    expect(modelWords(undefined)).toBeNull();
  });
});

describe("Markdown in a helper's words and report", () => {
  const LINKED =
    "See Sale_deed_2004.pdf and [the register](https://example.org/register) for **dates**.";

  test("the card's report reads as plain words: underscores kept, a link as its words", async () => {
    await room([
      helper(10, 1, {
        outcome: "done",
        finished_at: 70_000,
        summary: { inline: `${LINKED}\n\nMore.` },
      }),
    ]);
    const found = text(h!.one("helper-found"));
    expect(found).toContain("See Sale_deed_2004.pdf and the register for dates.");
    expect(found).not.toContain("https://");
    expect(found).not.toContain("**");
    expect(found).not.toContain("More.");
  });

  test("a failed helper's reason the same way", async () => {
    await room([
      helper(10, 1, { outcome: "failed", finished_at: 20_000, summary: { inline: LINKED } }),
    ]);
    const found = text(h!.one("helper-found"));
    expect(found).toContain("Sale_deed_2004.pdf and the register for dates.");
    expect(found).not.toContain("example.org");
  });

  test("its words, opened and flat, go through the answer's Markdown", async () => {
    await room([
      helper(10, 1),
      helperText(11, 1, 1, LINKED),
      helperStep(12, 60, 1, "completed"),
      helperText(13, 2, 9, "Flat **words** of a helper not loaded."),
    ]);
    expect(h!.one("helper-text-flat")!.querySelector("strong")?.textContent).toBe("words");
    await h!.click(cards()[0]!.querySelector("button"));
    const words = h!.one("helper-rows")!.querySelector(".helper-words")!;
    expect(words.querySelector("strong")?.textContent).toBe("dates");
    expect(words.textContent).toContain("the register");
    expect(words.textContent).not.toContain("https://");
    expect(words.textContent).toContain("Sale_deed_2004.pdf");
  });
});

describe("how it ended, live and after the answer", () => {
  test("ChatGPT's helper ended without a report names ChatGPT", async () => {
    await room(
      [helper(10, 1, { name: "Read documents", outcome: "failed", finished_at: 20_000 })],
      "idle",
      "codex-acp",
    );
    expect(text(h!.one("helper-found"))).toContain("It ended without reporting back to ChatGPT.");
  });

  test("Stop pressed: the working card turns stopped, with what it did by then", async () => {
    await room(
      [helper(10, 1), helperStep(11, 60, 1, "completed"), helperStep(12, 61, 1, "in_progress")],
      "running",
    );
    expect(cards()[0]!.dataset["state"]).toBe("working");
    h!.deliver([
      upsert(helperStep(12, 61, 1, "cancelled")),
      upsert(helper(10, 1, { outcome: "stopped", finished_at: 10_000 + 3 * 60_000 })),
      upsert(step(30, { stop: "cancelled" })),
    ]);
    await h!.settle();
    expect(cards()[0]!.dataset["state"]).toBe("stopped");
    expect(text(h!.one("helper-state"))).toBe("Stopped after 3 min");
    expect(text(h!.one("helper-found"))).toContain("By then: ");
    expect(h!.one("helper-now")).toBeNull();
  });

  test("a background helper of an ended turn reads working, in the background, no minutes counting", async () => {
    // #1289: a turn that ends by itself leaves its background helpers open; the host ends
    // them later
    await room([
      helper(10, 1),
      helperStep(11, 60, 1, "in_progress"),
      step(20, { ts: 10_000 + 2 * 60_000 }),
    ]);
    const card = cards()[0]!;
    expect(card.dataset["state"]).toBe("working");
    expect(card.hasAttribute("data-background")).toBe(true);
    expect(text(h!.one("helper-state"))).toBe(steps("en", "Working in the background"));
    expect(text(h!.one("helper-now"))).toContain("Reading Sale deed 60.pdf.");
  });

  test("it flips to done on its end frame", async () => {
    await room([
      helper(10, 1),
      helperStep(11, 60, 1, "completed"),
      step(20, { ts: 10_000 + 2 * 60_000 }),
    ]);
    expect(cards()[0]!.dataset["state"]).toBe("working");
    h!.deliver([
      upsert(
        helper(10, 1, {
          outcome: "done",
          finished_at: 10_000 + 5 * 60_000,
          summary: { inline: "The chain holds." },
        }),
      ),
    ]);
    await h!.settle();
    const card = cards()[0]!;
    expect(card.dataset["state"]).toBe("done");
    expect(card.hasAttribute("data-background")).toBe(false);
    expect(text(h!.one("helper-state"))).toBe("Took 5 min");
    expect(text(h!.one("helper-found"))).toContain("The chain holds.");
  });

  test("a real stopped outcome after the turn reads stopped", async () => {
    await room([
      helper(10, 1, { outcome: "stopped", finished_at: 10_000 + 60_000 }),
      helperStep(11, 60, 1, "cancelled"),
      step(20, { ts: 10_000 + 2 * 60_000 }),
    ]);
    expect(cards()[0]!.dataset["state"]).toBe("stopped");
    expect(text(h!.one("helper-state"))).toBe("Stopped after 1 min");
  });

  test("a foreground helper the host ended with its turn keeps that outcome", async () => {
    await room([
      helper(10, 1, { outcome: "failed", finished_at: 10_000 + 2 * 60_000 }),
      helperStep(11, 60, 1, "completed"),
      step(20, { ts: 10_000 + 2 * 60_000 }),
    ]);
    expect(cards()[0]!.dataset["state"]).toBe("failed");
    expect(cards()[0]!.hasAttribute("data-background")).toBe(false);
    expect(text(h!.one("helper-state"))).toBe("Failed after 2 min");
  });

  test("an entry with no steps: working says it is on it; done says it finished; nothing to open", async () => {
    await room([helper(10, 1)], "running");
    expect(text(h!.one("helper-now"))).toContain(stepLive("en", "Working on it."));
    expect(cards()[0]!.querySelector("button")!.disabled).toBe(true);
    h!.deliver([upsert(helper(10, 1, { outcome: "done", finished_at: 30_000 }))]);
    await h!.settle();
    expect(text(h!.one("helper-found"))).toBe(steps("en", "Finished."));
  });
});

describe("the working line counts only the running turn's helpers", () => {
  const view = (entries: ViewEntry[]): StreamView =>
    ({
      header: header("running"),
      ids: entries.map(entryIdOf),
      entries: new Map(entries.map((e) => [entryIdOf(e), e])),
    }) as unknown as StreamView;
  const OLD = "01900004-0000-7000-8000-0000000000aa";

  test("a helper left open by an older turn does not count", () => {
    const pick = doingPick("en");
    const entries = [
      helper(2, 9, { turn: OLD }),
      step(3, { turn: OLD }),
      step(4, { turn: "01900004-0000-7000-8000-0000000000bb" }),
      agentCall(10, 50, "in_progress", "Read"),
      helper(11, 1, { call: callId(50) }),
    ];
    expect(pick(view(entries))).toBe(stepLive("en", "Waiting for a helper to finish."));
  });

  test("a helper of the turn before, still working in the background, is not waited on", () => {
    // the lead of the running turn does not wait on it; its own card says it works
    const pick = doingPick("en");
    const entries = [
      helper(2, 9, { turn: OLD }),
      step(3, { turn: OLD }),
      agentCall(10, 50, "in_progress", "Read"),
      helper(11, 1, { call: callId(50) }),
    ];
    expect(pick(view(entries))).toBe(stepLive("en", "Waiting for a helper to finish."));
  });

  test("a lead working on its own does not get the waiting line", () => {
    const pick = doingPick("en");
    const launched = [agentCall(10, 50, "completed", "Read"), helper(11, 1, { call: callId(50) })];
    // its own step after the launch, done: no line; still in hand: what it does
    expect(pick(view([...launched, helperStep(12, 62, null, "completed")]))).toBeNull();
    const parallel = [helperStep(9, 63, null, "in_progress"), ...launched];
    expect(pick(view(parallel))).toBe("Reading Sale deed 63.pdf.");
  });
});

describe("a background helper with no Yield: one answer, the card between the lead's words", () => {
  // C2's recorded run L1 (claude-background-two-answers.jsonl): the launch, the lead's first
  // words, the helper working, its hand-back, then the digest, all in one turn
  const launch = agentCall(10, 50, "completed", "Read the deeds");
  const card = (over: Partial<HelperEntry> = {}) =>
    helper(11, 1, { call: callId(50), label: "Read the deeds", ...over });
  const first = agentText(14, 2, "I've started a helper on the deeds; I'll write once it reports.");
  const working = [launch, card(), helperStep(12, 60, 1, "completed"), first];
  const handedBack = [
    launch,
    card({ outcome: "done", finished_at: 40_000, summary: { inline: "The chain holds." } }),
    helperStep(12, 60, 1, "completed"),
    first,
    agentText(20, 3, "The helper found the chain holds from 1998."),
  ];

  test("the waiting line shows after the first words and clears once the helper hands back", () => {
    const pick = doingPick("en");
    expect(pick(viewOf([launch, card()]))).toBe(stepLive("en", "Waiting for a helper to finish."));
    expect(pick(viewOf(working))).toBe(stepLive("en", "Waiting for a helper to finish."));
    expect(pick(viewOf(handedBack))).toBeNull();
    // the lead's own step in hand after its first words still says what it does
    expect(pick(viewOf([...working, helperStep(15, 62, null, "in_progress")]))).toBe(
      "Reading Sale deed 62.pdf.",
    );
  });

  test("the card stands in the launch step's place: card, first words, digest", async () => {
    await room(handedBack);
    const order = [
      ...h!.host.querySelectorAll('[data-testid="helper-card"], [data-testid="message-agent"]'),
    ].map((el) =>
      el.getAttribute("data-testid") === "helper-card" ? "card" : text(el).slice(0, 12),
    );
    expect(order).toEqual(["card", "I've started", "The helper f"]);
    expect(cards()[0]!.dataset["state"]).toBe("done");
  });
});

describe("the opened words, live", () => {
  const LINK_LAST = "Found the deed.\n\nSee [the register](https://example.org/register)";

  test("a working helper's last words stream: their link stays inert until the helper is done", async () => {
    await room(
      [helper(10, 1), helperStep(11, 60, 1, "completed"), helperText(12, 1, 1, LINK_LAST)],
      "running",
    );
    await h!.click(cards()[0]!.querySelector("button"));
    const link = () =>
      h!
        .one("helper-rows")!
        .querySelector(".helper-words a, .helper-words [data-clauseo='link-text']");
    expect(link()?.hasAttribute("href")).toBe(false);
    h!.deliver([upsert(helper(10, 1, { outcome: "done", finished_at: 30_000 }))]);
    await h!.settle();
    if (!cards()[0]!.querySelector('[aria-expanded="true"]'))
      await h!.click(cards()[0]!.querySelector("button"));
    expect(h!.one("helper-rows")!.querySelector("a:not([href])")).toBeNull();
    expect(text(link())).toBe("the register");
  });

  test("the opened rows are a box of their own height at most, scrolled within and reachable by key", async () => {
    await room([helper(10, 1), helperStep(11, 60, 1, "completed")]);
    await h!.click(cards()[0]!.querySelector("button"));
    expect(h!.one("helper-rows")!.getAttribute("tabindex")).toBe("0");
    const rule = /\.helper-rows\s*\{([^}]*)\}/.exec(cardCss)?.[1] ?? "";
    expect(rule).toMatch(/max-height:/);
    expect(rule).toMatch(/overflow-y:\s*auto/);
  });

  test("words arriving while the card is closed draw nothing new", async () => {
    await room([helper(10, 1), helperStep(11, 60, 1, "in_progress")], "running");
    const before = cards()[0]!.innerHTML;
    h!.deliver([upsert(helperText(12, 1, 1, "A long report ".repeat(200)))]);
    await h!.settle();
    expect(h!.one("helper-rows")).toBeNull();
    expect(cards()[0]!.innerHTML).toBe(before);
  });
});
