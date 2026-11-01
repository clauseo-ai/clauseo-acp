// @vitest-environment happy-dom
// The rail, rendered with invented matters: date groups and Pinned, each status's words and dot, the current matter's
// conversations, finding a matter by name, client or case number, and the first launch with no matters.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { Rail, type FindableRow, type MatterRowProps, type RailGroup } from "./rail.tsx";
import { matterStatus } from "./rail-words.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  for (const n of document.body.querySelectorAll("[data-base-ui-portal]")) n.remove();
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
});

const txt = (n: Element | null | undefined) => (n?.textContent ?? "").replace(/\u00a0/g, " ");
const rows = () => [...document.querySelectorAll<HTMLButtonElement>(".room-matter-row")];
const row = (name: string) => rows().find((r) => txt(r).includes(name));

const acme: MatterRowProps = {
  name: "Acme Ltd v. State",
  note: "Recovery suit, for the plaintiff",
  hint: "Acme Ltd v. State",
};
const bharat: MatterRowProps = {
  name: "Bharat Mills v. Union of India",
  note: "Customs writ",
  hint: "Bharat Mills",
  state: "pausedSet",
  at: "6:10 pm",
};
const trust: MatterRowProps = {
  name: "Rao Family Trust",
  note: "Trust deed",
  hint: "Rao Family Trust",
  state: "paused",
  at: "6:10 pm",
};

describe("the rail's matters", () => {
  test("pinned matters stand above the date groups, and empty groups don't show", async () => {
    const groups: RailGroup[] = [
      { when: "Tomorrow", matters: [bharat] },
      { when: "Next week", matters: [] },
      { when: "No date", matters: [trust] },
    ];
    await mount(<Rail open groups={groups} pinned={[{ ...acme, pinned: true }]} />);
    expect([...document.querySelectorAll(".room-when")].map((h) => h.textContent)).toEqual([
      "Pinned",
      "Tomorrow",
      "No date",
    ]);
    expect(rows().map((r) => r.querySelector(".room-matter-name")?.textContent)).toEqual([
      acme.name,
      bharat.name,
      trust.name,
      "Scratch matter",
    ]);
  });

  test("a paused matter says until when, in yellow only while its choice is open", async () => {
    await mount(<Rail open groups={[{ when: "Today", matters: [bharat, trust] }]} />);
    const set = row("Bharat Mills");
    const open = row("Rao Family Trust");
    expect(set?.dataset["state"]).toBe("pausedSet");
    expect(txt(set)).toContain("Paused until 6:10 pm");
    expect(set?.querySelector(".bg-yellow")).toBeNull();
    expect(open?.dataset["state"]).toBe("paused");
    expect(txt(open)).toContain("Limit reached. Continue at 6:10 pm?");
    expect(open?.querySelector(".bg-yellow")).not.toBeNull();
  });

  test("an idle matter has no status line, and the words name the state's time", () => {
    expect(matterStatus("en", "idle", "")).toBeNull();
    expect(matterStatus("en", "working", "4:32 pm")).toBe("Answering. Expected by 4:32\u00a0pm");
    expect(matterStatus("en", "pausedSet", "")).toBe("Paused at the plan's limit");
    expect(matterStatus("en", "paused", "9:00 am", "Tuesday")).toBe(
      "Limit reached. Continue on Tuesday at 9:00\u00a0am?",
    );
    expect(matterStatus("en", "moved", "")).toBe("Folder moved");
  });

  test("a row pressed opens its matter, and New matter calls the host", async () => {
    const opened: string[] = [];
    let added = 0;
    await mount(
      <Rail
        open
        groups={[{ when: "Today", matters: [{ ...acme, onOpen: () => opened.push("acme") }] }]}
        onNewMatter={() => (added += 1)}
      />,
    );
    await act(async () => row("Acme")?.click());
    const add = [...document.querySelectorAll("button")].find(
      (b) => b.textContent === "New matter",
    );
    await act(async () => add?.click());
    expect(opened).toEqual(["acme"]);
    expect(added).toBe(1);
  });

  test("with no matters, the rail says so in one sentence, and only that", async () => {
    await mount(<Rail open groups={[{ when: "No date yet", matters: [] }]} />);
    expect(txt(document.body)).toContain("No matters yet");
    expect(txt(document.body)).toContain("Add a matter's folder.");
    expect(txt(document.body)).not.toContain("reads its papers");
    expect(document.querySelector(".room-groups")).toBeNull();
    expect(document.querySelector("[data-kind=scratch]")).not.toBeNull();
  });

  test("test ids land on New matter, the scratch row and each row, and rows key by id", async () => {
    await mount(
      <Rail
        open
        groups={[
          {
            when: "No date yet",
            matters: [
              { ...acme, id: "m1", testIds: { row: "matter-row" } },
              { ...acme, id: "m2", testIds: { row: "matter-row" } },
            ],
          },
        ]}
        testIds={{ newMatter: "new-matter", scratch: "scratch-matter" }}
      />,
    );
    expect(document.querySelector("[data-testid=new-matter]")?.textContent).toBe("New matter");
    expect(document.querySelector("[data-testid=scratch-matter]")?.getAttribute("data-kind")).toBe(
      "scratch",
    );
    // two matters of one name both stand, each with its state
    const found = document.querySelectorAll("[data-testid=matter-row]");
    expect(found).toHaveLength(2);
    expect(found[0]?.getAttribute("data-state")).toBe("idle");
  });

  test("a matter at the plan's limit says until when, muted, or that the reset wasn't given", async () => {
    const at = (state: "limit", t: string, day?: string): MatterRowProps => ({
      ...acme,
      name: `Limit ${t} ${day ?? ""}`,
      state,
      at: t,
      ...(day === undefined ? {} : { day }),
    });
    await mount(
      <Rail
        open
        groups={[
          {
            when: "Today",
            matters: [at("limit", "6:10 pm"), at("limit", "9:00 am", "Tuesday"), at("limit", "")],
          },
        ]}
      />,
    );
    const limits = rows().filter((r) => r.dataset["state"] === "limit");
    expect(limits.map((r) => txt(r.querySelector(".min-w-0")))).toEqual([
      "Limit reached until 6:10 pm",
      "Limit reached until Tuesday 9:00 am",
      "Limit reached. Reset not given.",
    ]);
    for (const r of limits) {
      expect(r.querySelector(".bg-muted")).not.toBeNull();
      expect(r.querySelector(".text-muted")).not.toBeNull();
    }
    expect(matterStatus("hi", "limit", "")).toBe("सीमा पूरी। रीसेट का समय नहीं बताया गया।");
  });

  test("a working matter with no end time says Answering.", () => {
    expect(matterStatus("en", "working", "")).toBe("Answering.");
    expect(matterStatus("hi", "working", "")).toBe("उत्तर दिया जा रहा है।");
  });

  test("in Hindi, the rail's words are Hindi and the matters' names stay as filed", async () => {
    showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced", lang: "hi" });
    await mount(<Rail open groups={[{ when: "आज", matters: [trust] }]} />);
    expect(txt(document.body)).toContain("Rao Family Trust");
    expect(txt(document.body)).toContain("कच्चा काम");
    expect(txt(document.body)).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
  });
});

describe("a matter's actions", () => {
  const current = (p: Partial<MatterRowProps>): MatterRowProps => ({
    ...acme,
    current: true,
    conversations: [],
    ...p,
  });
  const menuLabels = () =>
    [...document.querySelectorAll("[role=menuitem], [role=menuitemcheckbox]")].map((n) =>
      txt(n).trim(),
    );
  const open = async () => {
    const more = document.querySelector<HTMLButtonElement>(".room-house-more");
    await act(async () => {
      more?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0 }));
      more?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
      more?.click();
    });
  };

  test("with no handlers, a row has no menu: no chevron and no context menu", async () => {
    await mount(<Rail open groups={[{ when: "Today", matters: [current({}), trust] }]} />);
    expect(document.querySelector(".room-house-more")).toBeNull();
    const press = (r: HTMLButtonElement | undefined) =>
      act(async () => {
        r?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
      });
    await press(row("Rao Family Trust"));
    expect(menuLabels()).toEqual([]);
  });

  test("a right-click on a row with a handler opens its menu", async () => {
    await mount(
      <Rail open groups={[{ when: "Today", matters: [{ ...trust, onPin: () => {} }] }]} />,
    );
    await act(async () => {
      row("Rao Family Trust")?.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      );
    });
    expect(menuLabels()).toEqual(["Pin to the top"]);
  });

  test("each item shows only with its handler", async () => {
    const removed: string[] = [];
    await mount(
      <Rail
        open
        groups={[{ when: "Today", matters: [current({ onRemove: () => removed.push("acme") })] }]}
      />,
    );
    await open();
    expect(menuLabels()).toEqual([
      "Remove from the listThe folder and its papers stay where they are.",
    ]);
  });

  test("Locate folder stands only while the folder has moved; Show in Finder with onReveal", async () => {
    await mount(
      <Rail
        open
        groups={[
          {
            when: "Today",
            matters: [
              current({ state: "moved", onLocate: () => {}, onReveal: () => {}, onPin: () => {} }),
            ],
          },
        ]}
      />,
    );
    await open();
    expect(menuLabels()).toEqual(["Pin to the top", "Locate folder", "Show in Finder"]);
  });
});

describe("the current matter's conversations", () => {
  test("one line each; only a question says its words, and the plan shows only where it differs", async () => {
    const current: MatterRowProps = {
      ...acme,
      current: true,
      conversations: [
        { id: "a", title: "Reply to the notice", agent: "Claude", current: true },
        { id: "b", title: "Draft plaint", agent: "Claude", state: "question" },
        { id: "c", title: "Limitation", agent: "ChatGPT", state: "working", at: "4:41 pm" },
        { id: "d", title: "Annexures", agent: "Claude", state: "waiting", behind: "Draft plaint" },
      ],
    };
    await mount(<Rail open groups={[{ when: "Today", matters: [current] }]} />);
    const convs = [...document.querySelectorAll(".room-conv:not(.room-conv-new)")];
    expect(convs).toHaveLength(4);
    for (const c of convs) expect(c.querySelectorAll(".room-conv-line")).toHaveLength(1);
    expect([...document.querySelectorAll(".room-conv-ask")].map((n) => n.textContent)).toEqual([
      "A question for you",
    ]);
    expect([...document.querySelectorAll(".room-conv-plan")].map((n) => n.textContent)).toEqual([
      "ChatGPT",
    ]);
    expect(
      document.querySelector("[aria-label='Conversations in Acme Ltd v. State']"),
    ).not.toBeNull();
    expect(txt(convs[3])).toContain("Waiting for 'Draft plaint'");
    expect(document.querySelector(".room-conv-new")?.textContent).toBe("New conversation");
  });
});

describe("finding a matter", () => {
  const all: FindableRow[] = [
    { ...acme, client: "Acme Ltd", caseNo: "CC No. 1001/2024" },
    { ...trust, client: "Asha Rao", caseNo: "OS 77/2023" },
  ];
  const find = { all, recent: [acme] };

  test("the field stands on the sticky head; focused and empty it shows the recent matters", async () => {
    await mount(<Rail open groups={[{ when: "Today", matters: [trust] }]} find={find} />);
    expect(document.querySelector(".room-rail-head .room-find")).not.toBeNull();
    const input = document.querySelector<HTMLInputElement>(".room-find input");
    await act(async () => input?.focus());
    expect([...document.querySelectorAll(".room-when")].map((h) => h.textContent)).toEqual([
      "Recent",
    ]);
    expect(rows().map((r) => r.querySelector(".room-matter-name")?.textContent)).toContain(
      acme.name,
    );
  });

  test("words in the field find a matter by its client, and say why", async () => {
    await mount(
      <Rail open groups={[{ when: "Today", matters: [acme] }]} find={{ ...find, query: "asha" }} />,
    );
    expect([...document.querySelectorAll(".room-when")].map((h) => h.textContent)).toEqual([
      "Matching matters",
    ]);
    expect(txt(document.querySelector(".room-matter-match"))).toBe("Client, Asha Rao");
  });

  test("with no recent matters, the field focused and empty keeps the date groups", async () => {
    await mount(
      <Rail open groups={[{ when: "No date yet", matters: [trust] }]} find={{ all, recent: [] }} />,
    );
    const input = document.querySelector<HTMLInputElement>(".room-find input");
    await act(async () => input?.focus());
    expect([...document.querySelectorAll(".room-when")].map((h) => h.textContent)).toEqual([
      "No date yet",
    ]);
  });

  test("words that match nothing say so", async () => {
    await mount(
      <Rail
        open
        groups={[{ when: "Today", matters: [acme] }]}
        find={{ ...find, query: "zebra" }}
      />,
    );
    expect(txt(document.querySelector(".room-find-none"))).toContain("No matter matches 'zebra'.");
    expect(document.querySelector(".room-groups")).toBeNull();
  });
});
