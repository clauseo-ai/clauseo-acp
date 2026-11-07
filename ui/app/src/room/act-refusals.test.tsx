// @vitest-environment happy-dom The lawyer's open, show and find actions in the real room over
// FakeTransport, each refused: the refusal says its words by its code where the action came from (a
// rail row's note, the composer's line for Find's paper, the change note's and the wall's error
// lines); a declined confirm (transport.unauthorized) says nothing; a folder gone or not the
// matter's (matter.folder_missing, matter.folder_not_a_matter) offers Locate folder beside the
// words; and trying again clears it.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, describe, expect, test, vi } from "vitest";
import { MATTER, reset, step, uuid } from "../testing/stream-script.ts";
import type { FileChange, PaperSummary, SearchHit, ViewEntry } from "../transport/protocol.ts";
import { ActRefusals, locatable } from "./act-refusals.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import { AnswerHost } from "./open.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { mountRoom, summary, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

const refusal = (code: string) => ({ code, message: "for logs", retryable: false });
const DECLINED = "transport.unauthorized";
const MISSING = "matter.folder_missing";
const NOT_A_MATTER = "matter.folder_not_a_matter";
/** The codes each place is tried with: a known one, an unknown one (the generic words) and both
 * folder codes. */
const CODES = ["internal", "some.newer_code", MISSING, NOT_A_MATTER] as const;

const FILE = uuid(17, 8);
const PAPERS: PaperSummary[] = [{ file: FILE, path: "drafts/Reply.docx", size: 1, modified: 0 }];

test("Locate folder stands for exactly the two folder codes", () => {
  expect(locatable(MISSING)).toBe(true);
  expect(locatable(NOT_A_MATTER)).toBe(true);
  for (const code of ["internal", DECLINED, "matter.document_path_too_long", null])
    expect(locatable(code)).toBe(false);
});

describe("an answer's paper (AnswerHost)", () => {
  const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
  const host = (t: FakeTransport, refusals: ActRefusals) => {
    const answer = new AnswerHost(t, MATTER, refusals);
    answer.show({
      matter: MATTER,
      papers: { papers: PAPERS, skipped: [], unread: [], truncated: false },
    });
    return answer;
  };

  test("a refusal stands for the matter by its code; a decline says nothing; the next open clears it", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const refusals = new ActRefusals();
    let fail: string | null = NOT_A_MATTER;
    const t = new FakeTransport({
      openFile: () => {
        if (fail !== null) throw refusal(fail);
        return { protected_view: false };
      },
    });
    host(t, refusals).openReference("paper", FILE);
    await flush();
    expect(refusals.view().get(MATTER)).toEqual({ act: "open", code: NOT_A_MATTER });
    fail = DECLINED;
    host(t, refusals).openReference("paper", FILE);
    await flush();
    expect(refusals.view().has(MATTER)).toBe(false);
    fail = "internal";
    host(t, refusals).openReference("paper", FILE);
    await flush();
    expect(refusals.view().get(MATTER)?.code).toBe("internal");
    fail = null;
    host(t, refusals).openReference("paper", FILE);
    await flush();
    expect(refusals.view().has(MATTER)).toBe(false);
  });
});

/* ---------- Find's paper: the composer's line ---------- */

const paperHit: SearchHit = {
  kind: "file_name",
  matter: MATTER,
  matter_title: THE_MATTER.name,
  at: 0,
  path: "drafts/Reply.docx",
  snippet: [{ text: "Reply", hit: true }],
};

/** Find's room; with `listFails` set, list_papers is refused with it. */
let listFails: string | null = null;
async function findRoom(): Promise<Harness> {
  listFails = null;
  const next = fakeNext({
    matters: () => [detailed(THE_MATTER)],
    threads: () => [{ ...summary(), seen: true }],
    papers: PAPERS,
  });
  return mountRoom({
    next: {
      ...next,
      query: (q) => {
        if (listFails !== null && typeof q === "object" && "list_papers" in q)
          throw refusal(listFails);
        return next.query(q);
      },
    },
    query: (q) => {
      if (typeof q === "object" && "search" in q)
        return { search: { page: { hits: [paperHit], mode: "ranked" } } };
      throw new Error("not scripted");
    },
  });
}

async function openFound(room: Harness) {
  await act(async () => {
    const input = room.host.querySelector<HTMLInputElement>('[data-testid="find"]');
    input?.focus();
    if (input !== null) Reflect.set(HTMLInputElement.prototype, "value", "reply", input);
    input?.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => settled());
  await room.settle();
  await room.click(room.one("find-paper"));
  await room.settle();
  await room.settle();
}

const composerError = () => h!.one("error");

describe("Find's paper, refused, says why on the composer's line", () => {
  for (const code of CODES)
    test(`by its code, with Locate folder for a folder code, until the next try: ${code}`, async () => {
      h = await findRoom();
      vi.spyOn(console, "warn").mockImplementation(() => {});
      h.transport.host.openFile = () => {
        throw refusal(code);
      };
      await openFound(h);
      expect(h.transport.only("open_file")).toEqual([{ call: "open_file", file: FILE }]);
      expect(composerError()?.textContent).toContain(errorWords("en", code));
      expect(h.one("locate-folder") !== null).toBe(locatable(code));
      if (locatable(code)) {
        await h.click(h.one("locate-folder"));
        expect(h.transport.only("pick_folder")).toHaveLength(1);
      }
      // found and opened again, it goes through: the line is gone
      h.transport.host.openFile = () => ({ protected_view: false });
      await openFound(h);
      expect(composerError()).toBeNull();
      expect(h.one("locate-folder")).toBeNull();
    });

  test("a declined confirm says nothing", async () => {
    h = await findRoom();
    h.transport.host.openFile = () => {
      throw refusal(DECLINED);
    };
    await openFound(h);
    expect(h.transport.only("open_file")).toHaveLength(1);
    expect(composerError()).toBeNull();
  });

  test("a list that can't be read says why too", async () => {
    h = await findRoom();
    vi.spyOn(console, "warn").mockImplementation(() => {});
    listFails = NOT_A_MATTER;
    await openFound(h);
    expect(h.transport.only("open_file")).toEqual([]);
    expect(composerError()?.textContent).toContain(errorWords("en", NOT_A_MATTER));
    expect(h.one("locate-folder")).not.toBeNull();
  });
});

/* ---------- the rail row's Show in Finder ---------- */

/** A matter beside the open one: its row's menu is the context menu. */
const OTHER = { matter: uuid(5, 2), name: "Kapoor v. Kapoor", kind: "folder" } as const;

async function railRoom(): Promise<Harness> {
  const room = await mountRoom({
    next: fakeNext({
      matters: () => [detailed(THE_MATTER, { last_opened: 10 }), detailed(OTHER)],
      threads: () => [],
    }),
  });
  await act(async () => settled());
  await room.settle();
  return room;
}

const row = () =>
  [...(h?.host.querySelectorAll<HTMLElement>(".room-matter-row") ?? [])].find(
    (r) => r.querySelector(".room-matter-name")?.textContent === OTHER.name,
  );
const note = () => row()?.querySelector(".room-matter-note")?.textContent ?? "";

async function menu(): Promise<HTMLElement[]> {
  await act(async () =>
    row()?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })),
  );
  await act(async () => settled());
  await h!.settle();
  return [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')];
}

const escape = async () => {
  await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
  await h!.settle();
};

async function press(label: RegExp) {
  const item = (await menu()).find((i) => label.test(i.textContent ?? ""));
  if (item === undefined) throw new Error(`no ${String(label)} in the menu`);
  await act(async () => item.click());
  await h!.settle();
}

describe("a rail row's refused Show in Finder says why on its note line", () => {
  for (const code of CODES)
    test(`by its code, with Locate folder in the menu for a folder code, until the next try: ${code}`, async () => {
      h = await railRoom();
      vi.spyOn(console, "warn").mockImplementation(() => {});
      h.transport.host.reveal = () => {
        throw refusal(code);
      };
      const before = (await menu()).map((i) => i.textContent ?? "");
      expect(before.some((t) => /Locate folder/.test(t))).toBe(false);
      await escape();
      await press(/^Show in/);
      expect(h.transport.only("reveal")).toEqual([
        { call: "reveal", target: { matter: OTHER.matter } },
      ]);
      expect(note()).toBe(errorWords("en", code));
      const after = (await menu()).map((i) => i.textContent ?? "");
      expect(after.some((t) => /Locate folder/.test(t))).toBe(locatable(code));
      // Show in Finder stays pressable, to try again; a show that goes through clears the line
      h.transport.host.reveal = () => {};
      await escape();
      await press(/^Show in/);
      expect(note()).toBe("");
    });

  test("a declined confirm says nothing", async () => {
    h = await railRoom();
    h.transport.host.reveal = () => {
      throw refusal(DECLINED);
    };
    await press(/^Show in/);
    expect(h.transport.only("reveal")).toHaveLength(1);
    expect(note()).toBe("");
  });
});

/* ---------- the change note and the wall: Open in Word ---------- */

const change = (path: string): FileChange => ({
  path,
  change: "modified",
  size_bytes: 10,
  may_include_lawyer_edits: false,
});
const changed = (): ViewEntry =>
  step(9, { files: [change("drafts/Reply.docx")], checkpoint_after: uuid(1, 9), undone: false });

async function wordRoom(): Promise<Harness> {
  const room = await mountRoom({
    next: fakeNext({ matters: () => [], papers: PAPERS, parts: ["papers"] }),
  });
  await act(settled);
  await room.settle();
  return room;
}

const noteError = () =>
  h!.one("files-changed")?.querySelector<HTMLElement>('[data-testid="error"]') ?? null;
const noteLocate = () =>
  h!.one("files-changed")?.querySelector<HTMLElement>('[data-testid="locate-folder"]') ?? null;
const wallError = () =>
  h!.host.querySelector<HTMLElement>('aside.room-pane [data-testid="wall-error"]');
const wallLocate = () =>
  h!.host.querySelector<HTMLElement>('aside.room-pane [data-testid="locate-folder"]');
const tile = () => h!.host.querySelector<HTMLElement>("aside.room-pane button.tile-face");

for (const code of [MISSING, NOT_A_MATTER])
  test(`Open in Word refused with ${code}: the note's and the wall's error lines offer Locate folder`, async () => {
    h = await wordRoom();
    vi.spyOn(console, "warn").mockImplementation(() => {});
    h.transport.host.openFile = () => {
      throw refusal(code);
    };
    h.deliver([reset([changed()])]);
    await h.click(h.one("files-changed")?.querySelector('[data-testid="open-in-word"]'));
    await h.settle();
    expect(noteError()?.textContent).toBe(errorWords("en", code));
    expect(noteLocate()?.textContent).toBe("Locate folder");
    await h.click(noteLocate());
    expect(h.transport.only("pick_folder")).toHaveLength(1);
    await h.click(tile());
    await h.settle();
    expect(wallError()?.dataset["code"]).toBe(code);
    expect(wallLocate()?.textContent).toBe("Locate folder");
    // tried again, each goes through and its line goes
    h.transport.host.openFile = () => ({ protected_view: false });
    await h.click(tile());
    await h.settle();
    expect(wallError()).toBeNull();
    expect(wallLocate()).toBeNull();
    await h.click(h.one("files-changed")?.querySelector('[data-testid="open-in-word"]'));
    await h.settle();
    expect(noteError()).toBeNull();
    expect(noteLocate()).toBeNull();
  });

test("the note's Show in folder, refused, says why in the note's error line; declined, the line stays", async () => {
  h = await wordRoom();
  vi.spyOn(console, "warn").mockImplementation(() => {});
  h.transport.host.openFile = () => {
    throw refusal("matter.document_path_too_long");
  };
  h.deliver([reset([changed()])]);
  await h.click(h.one("files-changed")?.querySelector('[data-testid="open-in-word"]'));
  await h.settle();
  const reveal = () => h!.one("files-changed")?.querySelector('[data-testid="reveal"]');
  h.transport.host.reveal = () => {
    throw refusal(DECLINED);
  };
  await h.click(reveal());
  await h.settle();
  expect(noteError()?.dataset["code"]).toBe("matter.document_path_too_long");
  h.transport.host.reveal = () => {
    throw refusal(NOT_A_MATTER);
  };
  await h.click(reveal());
  await h.settle();
  expect(noteError()?.dataset["code"]).toBe(NOT_A_MATTER);
  expect(noteLocate()).not.toBeNull();
});
