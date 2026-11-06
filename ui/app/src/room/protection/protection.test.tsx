// @vitest-environment happy-dom
// UC in the room, over FakeTransport with the protection wire: the room's line under the composer for each state and
// both plans, the toolbar's and the rail's marks, a protection_changed swapping them, and the Not protected note for
// each cause (the pool's counts only, never a name), opening by itself once per computer (set-up, can't) or once per
// matter (switched off) across a remount, never for a full pool, and on a press of the line or the mark.
import { tbt } from "@clauseo/components/lib/words-toolbar.ts";
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { vt } from "@clauseo/components/lib/words-versions.ts";
import { rt } from "@clauseo/components/lib/words-room.ts";
import { act } from "react";
import { afterEach, beforeEach, expect, test } from "vitest";
import { MATTER, uuid } from "../../testing/stream-script.ts";
import type { MatterSummary, Protection } from "../../transport/protocol.ts";
import { np, PROT, view } from "./fixtures.ts";
import { loadProtectionNote } from "./note-load.ts";
import { mountRoom, summary, THE_MATTER, type Harness, type Script } from "../thread/harness.ts";

let h: Harness | null = null;
beforeEach(() => localStorage.clear());
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

/** Notes already explained, so none opens by itself. */
const explained = (matters: string[] = [MATTER]) =>
  localStorage.setItem("clauseo.protection-explained", JSON.stringify({ computer: true, matters }));

const mount = async (protection: Protection | undefined, over: Partial<Script> = {}) => {
  const m: MatterSummary = { ...THE_MATTER, ...(protection === undefined ? {} : { protection }) };
  h = await mountRoom({
    matters: [m],
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    protection: { view: () => view(protection ?? PROT.a) },
    ...over,
  });
  await waitNote();
  return h;
};

/** Lets the lazy note chunk land. */
const waitNote = async () => {
  for (let i = 0; i < 5; i += 1) await h!.settle();
};

const line = () => h!.one("workspace-line");
const safeLine = () => h!.host.querySelector(".room-line")?.textContent ?? null;
const SAFE = rt(
  "en",
  "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.",
);
const note = () => document.body.querySelector<HTMLElement>('[data-testid="np-note"]');
const toolbarMark = () => h!.host.querySelector(".room-next .np-mark");
const railMark = (name = THE_MATTER.name) =>
  h!
    .all("matter-row")
    .find((r) => r.textContent?.includes(name))
    ?.querySelector(".room-matter-np") ?? null;

const CLAUDE_LINE = tbt("en", "Clauseo asks before each change in this matter.");
const CODEX_LINE = tbt(
  "en",
  "Clauseo can change files in this matter without asking. It asks before going beyond it.",
);
const OFF_LINE = tbt("en", "Clauseo can reach beyond this matter's folder without asking.");

test("a matter whose protection isn't known shows no line and no mark (QU.1)", async () => {
  await mount(undefined);
  expect(line()).toBeNull();
  expect(h!.host.querySelector(".room-line")).toBeNull();
  expect(toolbarMark()).toBeNull();
  expect(railMark()).toBeNull();
});

test.each(["a", "b"] as const)(
  "protected at tier %s: the composer's Safe workspace line, not pressable, and no mark",
  async (tier) => {
    await mount(PROT[tier]);
    expect(line()).toBeNull();
    expect(safeLine()).toBe(SAFE);
    expect(h!.host.querySelector(".room-line button")).toBeNull();
    expect(toolbarMark()).toBeNull();
    expect(railMark()).toBeNull();
  },
);

const NP_CASES = [
  ["needs", "needs_set_up"],
  ["refused", "set_up_refused"],
  ["failed", "set_up_failed"],
  ["unsupported", "unsupported"],
  ["newer", "unknown"],
  ["pool", "pool_full"],
] as const;

for (const [plan, agent, words] of [
  ["Claude", "claude-acp", CLAUDE_LINE],
  ["ChatGPT", "codex-acp", CODEX_LINE],
] as const)
  test.each(NP_CASES)(
    `%s on ${plan}: Not protected, in ${plan}'s words, and both marks`,
    async (key, cause) => {
      explained();
      await mount(PROT[key], { threads: [{ ...summary(), agent }] });
      expect(line()?.dataset["protection"]).toBe("not_protected");
      expect(line()?.dataset["cause"]).toBe(cause);
      expect(line()?.textContent).toBe(`${tbt("en", "Not protected.")} ${words}`);
      expect(line()?.textContent).not.toContain(plan === "Claude" ? CODEX_LINE : CLAUDE_LINE);
      expect(toolbarMark()?.textContent).toBe(tbt("en", "Not protected"));
      expect(railMark()).not.toBeNull();
    },
  );

test.each(["claude-acp", "codex-acp"])(
  "switched off on %s: the mark's own line, Clauseo reaches beyond the folder without asking",
  async (agent) => {
    explained();
    await mount(PROT.off, { threads: [{ ...summary(), agent }] });
    expect(line()?.dataset["cause"]).toBe("switched_off");
    expect(line()?.textContent).toBe(`${tbt("en", "Not protected.")} ${OFF_LINE}`);
  },
);

test("a press of the line, or of the toolbar's mark, opens that cause's note", async () => {
  explained();
  await mount(PROT.refused);
  expect(note()).toBeNull();
  await h!.click(line());
  await waitNote();
  expect(note()?.dataset["cause"]).toBe("set_up_refused");
  await h!.click(document.body.querySelector(".np-note button:last-child"));
  expect(note()).toBeNull();
  await h!.click(h!.one("protection-mark"));
  await waitNote();
  expect(note()?.dataset["cause"]).toBe("set_up_refused");
});

test("keys: the line says it opens a dialog; the note takes focus, and Escape closes it and gives focus back", async () => {
  explained();
  await mount(PROT.refused);
  expect(line()?.getAttribute("aria-haspopup")).toBe("dialog");
  expect(h!.one("protection-mark")?.getAttribute("aria-haspopup")).toBe("dialog");
  line()?.focus();
  await h!.click(line());
  await waitNote();
  expect(note()?.contains(document.activeElement)).toBe(true);
  await act(async () => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
  });
  await waitNote();
  await act(async () => new Promise((resolve) => setTimeout(resolve, 80)));
  expect(note()).toBeNull();
  expect(document.activeElement).toBe(line());
});

test("the toolbar's mark shows for a folder reason or a Not protected matter, not for protected", async () => {
  explained();
  await mount(PROT.a, {
    matters: [{ ...THE_MATTER, protection: PROT.a, folder_support: "network" }],
  });
  expect(toolbarMark()).not.toBeNull();
  // the folder's reason isn't protection's: no press into the note
  expect(h!.one("protection-mark")).toBeNull();
});

test("protection_changed for the open matter swaps the line and both marks in one commit; another matter's changes only its row", async () => {
  explained([MATTER, uuid(9, 2)]);
  const other: MatterSummary = {
    ...THE_MATTER,
    matter: uuid(9, 2),
    name: "Rathi Steels v. Northgate Infra",
    protection: PROT.a,
  };
  const commits: string[] = [];
  await mount(PROT.a, {
    matters: [{ ...THE_MATTER, protection: PROT.a }, other],
    threads: (m) => [{ ...summary(uuid(m === other.matter ? 9 : 1, 3)), matter: m }],
    // each commit's line and toolbar mark, as the page holds them then
    onRender: () => {
      if (h !== null)
        commits.push(
          `${line()?.dataset["cause"] ?? "protected"} ${toolbarMark() === null ? "no-mark" : "mark"}`,
        );
    },
  });
  const open = h!.one("matter")?.textContent;
  const [mine, theirs] = open === other.name ? [other.matter, MATTER] : [MATTER, other.matter];
  const theirName = open === other.name ? THE_MATTER.name : other.name;
  expect(safeLine()).toBe(SAFE);
  await act(async () => void (await loadProtectionNote()));
  commits.length = 0;
  await act(async () => h!.transport.protectionChanged(mine, PROT.refused));
  // the line and the mark change together, in the first commit (the note's lazy part may mount after it)
  expect(commits[0]).toBe("set_up_refused mark");
  expect(line()?.dataset["cause"]).toBe("set_up_refused");
  expect(toolbarMark()).not.toBeNull();
  expect(railMark(open)).not.toBeNull();
  expect(railMark(theirName)).toBeNull();
  await h!.settle();
  await act(async () => h!.transport.protectionChanged(theirs, PROT.off));
  expect(railMark(theirName)).not.toBeNull();
  expect(line()?.dataset["cause"]).toBe("set_up_refused");
  // the view is read again after each, never polled
  expect(h!.transport.only("query").filter((c) => c.query === "protection")).toHaveLength(3);
});

const noteText = () => note()?.textContent ?? "";

test.each([
  ["needs", "needs_set_up", "Protection isn't set up on this computer yet."],
  ["refused", "set_up_refused", "Protection couldn't be set up on this computer"],
  ["failed", "set_up_failed", "Protection couldn't be set up on this computer"],
  ["unsupported", "cant", "This computer can't give Clauseo's protection"],
  ["newer", "cant", "This computer can't give Clauseo's protection"],
  ["off", "off", "Protection is off for Sharma v. State of Delhi."],
] as const)("the note for %s says its cause, and what is still kept", async (key, why, words) => {
  await mount(PROT[key]);
  expect(note()?.dataset["cause"]).toBe(why);
  expect(noteText()).toContain(words);
  expect(noteText()).toContain(
    vt("en", "Every change inside the matter's folder is still kept, and Versions can undo it."),
  );
  expect(noteText()).not.toContain(vt("en", "Switch protection on"));
  // never a promise to ask before each change (Changed, 11)
  expect(noteText()).not.toContain("asks before each change");
  const buttons = [...note()!.querySelectorAll("button")].map((b) => b.textContent);
  expect(buttons.at(-1)).toBe(vt("en", "Understood"));
  expect(buttons.length).toBe(why === "off" || why === "cant" ? 1 : 2);
});

test("the pool's note: counts only, one line per nonzero count, and its ways out", async () => {
  await mount(PROT.pool);
  await h!.click(line());
  await waitNote();
  const text = noteText();
  expect(text).toContain(
    pw(
      "en",
      "All {size} of this computer's protected spaces are in use, so this matter works without protection for now. It's protected again once one is free.",
      { size: 8 },
    ),
  );
  const counts = [...document.body.querySelectorAll('[data-testid="np-pool"] > span')].map(
    (s) => s.textContent,
  );
  expect(counts).toEqual([
    pw("en", "In use by other open matters: {n}", { n: 5 }),
    pw("en", "Freed the next time Clauseo starts work: {n}", { n: 2 }),
    pw("en", "Still holding files set aside: {n}", { n: 1 }),
  ]);
  expect(text).not.toContain("Kept by matters with nothing open");
  expect(text).toContain(pw("en", "To free one now, end a conversation in another matter."));
  expect(text).toContain(
    pw("en", "Clauseo can add room for 4 more. Windows asks for permission first."),
  );
  expect(h!.one("protection-setup")).toBeNull();
  expect(document.body.querySelector('[data-testid="protection-setup"]')?.textContent).toBe(
    pw("en", "Add room"),
  );
  // no name or SID, anywhere
  expect(document.body.textContent).not.toMatch(/S-1-5-|clauseo-agent|sandbox/i);
});

test("a pool at its largest offers no Add room", async () => {
  await mount(np({ pool_full: { size: 32, open: 0, held: 32, owed: 0, set_aside: 0 } }));
  await h!.click(line());
  await waitNote();
  expect(noteText()).toContain(pw("en", "Kept by matters with nothing open: {n}", { n: 32 }));
  expect(noteText()).not.toContain(
    pw("en", "To free one now, end a conversation in another matter."),
  );
  expect(document.body.querySelector('[data-testid="protection-setup"]')).toBeNull();
});

test("the note opens by itself once per computer for set-up and can't, across a remount", async () => {
  await mount(PROT.needs);
  expect(note()?.dataset["cause"]).toBe("needs_set_up");
  await h!.unmount();
  document.body.replaceChildren();
  await mount(PROT.unsupported);
  expect(note()).toBeNull();
});

test("switched off opens by itself once per matter, across a remount; a full pool never does", async () => {
  await mount(PROT.off);
  expect(note()?.dataset["cause"]).toBe("off");
  await h!.unmount();
  document.body.replaceChildren();
  await mount(PROT.off);
  expect(note()).toBeNull();
  await h!.unmount();
  document.body.replaceChildren();
  localStorage.clear();
  await mount(PROT.pool);
  expect(note()).toBeNull();
});

test("a build without protection (unsupported { why: not_built }) reads calmly: the line and the mark, and the note only on a press", async () => {
  await mount(np({ unsupported: { why: "not_built" } }));
  expect(line()?.dataset["cause"]).toBe("unsupported");
  expect(note()).toBeNull();
  await h!.click(line());
  await waitNote();
  expect(note()?.dataset["cause"]).toBe("not_built");
  expect(noteText()).toContain(
    pw(
      "en",
      "This build of Clauseo comes without protection, so Clauseo can read any file on this computer. It asks before changing anything outside this matter's folder.",
    ),
  );
  expect(document.body.querySelector('[data-testid="protection-setup"]')).toBeNull();
});
