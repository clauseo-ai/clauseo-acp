// @vitest-environment happy-dom
// The parts of Versions, rendered with an invented history: the timeline reads with many versions, the band and its
// waiting acts, the restore dialog naming the time and the files, the sheets' blackline and a PDF, the wall as it
// stood, and where the history lives.
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { MatterContext } from "./matter-context.ts";
import type { MatterData } from "./matter.ts";
import { RestoreDialog } from "./restore-dialog.tsx";
import { VersionBand } from "./version-band.tsx";
import { DatesSheet, DraftSheet, PdfSheet } from "./version-page.tsx";
import { VersionPanel } from "./version-panel.tsx";
import { VersionWall } from "./version-wall.tsx";
import { VersionWhere } from "./version-where.tsx";
import { diffParagraphs, diffRows } from "./versions-diff.ts";
import { CAL, DATES_REVS, DRAFT_REVS, HISTORY, ROWS, at, nameOf } from "./versions-fixture.ts";
import { restoreWords, undoWords, waitingLine } from "./versions-lines.ts";
import { asItStood, whoOf } from "./versions-model.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (typeof globalThis.matchMedia !== "function") {
  Object.defineProperty(globalThis, "matchMedia", {
    value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
  });
}

let root: ReturnType<typeof createRoot> | null = null;

async function show(node: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <StrictMode>
        <TooltipProvider>{node}</TooltipProvider>
      </StrictMode>,
    ),
  );
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

const one = <T extends Element>(selector: string, box: ParentNode = document) => {
  const n = box.querySelector<T>(selector);
  if (n === null) throw new Error(`no ${selector}`);
  return n;
};
const click = async (n: Element | undefined) => {
  if (n === undefined) throw new Error("nothing to press");
  await act(async () => (n as HTMLElement).click());
};
const button = (text: string, box: ParentNode = document) =>
  [...box.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text);
const words = () => document.body.textContent ?? "";
const last = HISTORY.length - 1;

test("the timeline reads with many versions: days hold their heads, runs fold, older days fold under one line", async () => {
  const onPick = vi.fn();
  await show(
    <VersionPanel
      history={HISTORY}
      calendar={CAL}
      name={nameOf}
      at={last}
      onPick={onPick}
      onClose={() => {}}
    />,
  );
  expect(one(".ver-panel .wall-count").textContent).toBe(String(HISTORY.length));
  expect(document.querySelectorAll(".ver-entry").length).toBeLessThan(HISTORY.length / 2);
  expect(document.querySelectorAll(".ver-day").length).toBe(4);
  expect(one(".ver-run-fold").textContent?.replace(/\u00a0/g, " ")).toBe(
    "9 more · 4:12 pm to 4:28 pm",
  );
  // Clauseo is the actor; the plan stands muted and last, and the filter names Clauseo
  const drafted = [...document.querySelectorAll(".ver-entry")].find((e) =>
    e.querySelector(".ver-entry-who")?.textContent?.startsWith("Clauseo, in Draft"),
  );
  expect(drafted?.querySelector(".ver-entry-plan")?.textContent).toBe("\u00a0·\u00a0Claude");
  expect(words()).toContain("By Clauseo");
  expect(words()).not.toContain("By Claude");
  expect(one(".ver-older").textContent).toContain("Earlier, 1 to 20 March");
  await click(one(".ver-older"));
  expect(document.querySelectorAll(".ver-day").length).toBe(12);
  // a folded run opens
  const fold = one(".ver-run-fold");
  expect(fold.getAttribute("aria-expanded")).toBe("false");
  await click(fold);
  expect(one(".ver-run-fold").getAttribute("aria-expanded")).toBe("true");
  // only the lawyer's own versions
  await click(button("By you"));
  for (const e of document.querySelectorAll(".ver-entry-who"))
    expect(e.textContent).not.toMatch(/^Clauseo/);
  expect(words()).toContain("You, outside Clauseo");
});

test("now says Now, its time stands in the tooltip, and choosing a version picks its place in history", async () => {
  const onPick = vi.fn();
  await show(
    <VersionPanel
      history={HISTORY}
      calendar={CAL}
      name={nameOf}
      at={last}
      onPick={onPick}
      onClose={() => {}}
    />,
  );
  const now = one('.ver-entry[aria-current="true"]');
  expect(now.textContent).toContain("NowYou, outside Clauseo");
  expect(now.textContent).not.toContain("4:58 pm");
  expect(now.getAttribute("title")).toBe("4:58 pm");
  const v8 = [...document.querySelectorAll(".ver-entry")].find((b) =>
    (b.textContent ?? "").startsWith("4:31 pm"),
  );
  await click(v8);
  expect(onPick).toHaveBeenCalledWith(HISTORY.indexOf(at("v8")));
  expect(onPick).not.toHaveBeenCalledWith(last);
});

test("a version that couldn't be kept stands at the top of the timeline", async () => {
  await show(
    <VersionPanel
      history={HISTORY}
      calendar={CAL}
      name={nameOf}
      at={last}
      onPick={() => {}}
      onClose={() => {}}
      unsaved={{
        at: "5:10 pm",
        who: "Clauseo, in Draft",
        why: "Not kept as a version. Only 96 MB free on this computer.",
      }}
    />,
  );
  expect(one(".ver-unsaved").textContent).toContain("Not kept as a version.");
});

const look = (id: string) => {
  const v = at(id);
  return {
    line: asItStood("en", CAL, v),
    who: whoOf("en", CAL, v, HISTORY),
    did: "",
    comparing: false,
    undoable: true,
  };
};
const band = {
  draft: { title: "Draft reply", pages: 4 },
  onCompare: () => {},
  onRestore: () => {},
  onUndo: () => {},
  onBack: () => {},
};

test("the band says when the room stands, and Back to now carries no key cap", async () => {
  await show(<VersionBand {...band} look={look("v4")} waiting={null} />);
  expect(one(".ver-line").textContent).toBe("The matter as it stood at 3:42 pm on 9 March");
  expect(one(".ver-back").querySelector(".kbd-cap, kbd")).toBeNull();
  expect(one(".ver-back").getAttribute("aria-keyshortcuts")).toBe("Escape");
  expect(document.querySelector(".ver-actions .ver-open-panel")).toBeNull();
});

test("while an answer runs, Restore and Undo wait, and say until when", async () => {
  const onRestore = vi.fn();
  await show(
    <VersionBand
      {...band}
      onRestore={onRestore}
      look={look("v4")}
      waiting={waitingLine("en", "Draft", "5:16 pm")}
    />,
  );
  expect(one(".ver-restore").getAttribute("aria-disabled")).toBe("true");
  expect(one(".ver-band .ver-wait").textContent).toBe(
    "Restore and Undo wait until the answer in Draft is done, expected by 5:16 pm.",
  );
  await click(one(".ver-restore"));
  expect(onRestore).not.toHaveBeenCalled();
});

test("at now the band is the draft's bar, with what just happened under it", async () => {
  await show(
    <VersionBand
      {...band}
      look={null}
      waiting={null}
      notices={[{ tone: "done", text: "Restored to 3:42 pm on 9 March. 3 documents changed." }]}
    />,
  );
  expect(one(".turn-what").textContent).toContain("Draft reply");
  expect(one(".turn-page").textContent).toBe("Page 1 of 4");
  expect(one(".ver-notice").textContent).toContain("Restored to 3:42 pm on 9 March.");
});

test("the restore dialog names the time and the files, draws Restore as the dark primary, and focus starts on it", async () => {
  const r = restoreWords("en", CAL, HISTORY, "v4", nameOf);
  if (r === null) throw new Error("no words");
  const onRestore = vi.fn();
  await show(
    <RestoreDialog
      ask={{ kind: "confirm", act: "restore", ...r, edited: [], editedHead: null }}
      waiting={null}
      onClose={() => {}}
      onChoice={() => {}}
      onRestore={onRestore}
      onTryAgain={() => {}}
    />,
  );
  const dialog = one('[role="dialog"]');
  expect(dialog.textContent).toContain("Restore the matter to how it was at 3:42 pm on 9 March?");
  expect(dialog.textContent).toContain(
    "3 documents will change: Draft reply, List of dates and Junior's note. You can undo this.",
  );
  expect(dialog.textContent).toContain("'Letter from the bank', added since, stays.");
  const go = one(".ver-do-restore", dialog);
  expect(go.className).toContain("bg-primary");
  // Base UI moves focus in on the next frame
  await vi.waitFor(() => expect(document.activeElement).toBe(go));
  await click(go);
  expect(onRestore).toHaveBeenCalledTimes(1);
});

test("while an answer runs the dialog's act waits", async () => {
  const r = restoreWords("en", CAL, HISTORY, "v4", nameOf);
  if (r === null) throw new Error("no words");
  const onRestore = vi.fn();
  const waiting = waitingLine("en", "Draft", "5:16 pm");
  const ask = { kind: "confirm", ...r, edited: [], editedHead: null } as const;
  await show(
    <RestoreDialog
      ask={ask}
      waiting={waiting}
      onClose={() => {}}
      onChoice={() => {}}
      onRestore={onRestore}
      onTryAgain={() => {}}
    />,
  );
  await click(one(".ver-do-restore"));
  expect(onRestore).not.toHaveBeenCalled();
  expect(one('[role="dialog"]').textContent).toContain(waiting);
});

test("Word holding the file: close it in Word, and Try again", async () => {
  const onTryAgain = vi.fn();
  await show(
    <RestoreDialog
      ask={{ kind: "word", file: "Draft reply.docx" }}
      waiting={null}
      onClose={() => {}}
      onChoice={() => {}}
      onRestore={() => {}}
      onTryAgain={onTryAgain}
    />,
  );
  expect(one('[role="dialog"]').textContent).toContain(
    "Close 'Draft reply.docx' in Word to restore it.",
  );
  await click(one(".ver-try"));
  expect(onTryAgain).toHaveBeenCalledTimes(1);
});

test("undoing a change asks about each paper changed again since, Undo anyway or Keep current", async () => {
  const u = undoWords("en", CAL, HISTORY, "v8", ["draft"], nameOf);
  if (u === null) throw new Error("no words");
  const onChoice = vi.fn();
  await show(
    <RestoreDialog
      ask={{
        kind: "confirm",
        act: "undo",
        ...u,
        stays: null,
        edited: u.edited.map((e) => ({ ...e, choice: "keep" as const })),
      }}
      waiting={null}
      onClose={() => {}}
      onChoice={onChoice}
      onRestore={() => {}}
      onTryAgain={() => {}}
    />,
  );
  const dialog = one('[role="dialog"]');
  expect(dialog.textContent).toContain("Undo the change Clauseo made at 4:31 pm today?");
  expect(dialog.textContent).toContain("Changed by you, outside Clauseo, at 4:58 pm");
  expect(dialog.textContent).toContain(
    "1 document goes back to how it was just before it: List of dates.",
  );
  const row = one(".ver-edited-row");
  expect(row.textContent).toMatch(/Undo anyway.*Keep current/);
  await click(button("Undo anyway", row));
  expect(onChoice).toHaveBeenCalledWith("draft", "undo");
  expect(one(".ver-do-restore").textContent).toBe("Undo");
});

test("the sheets: the blackline keeps the lawyer's weight, a PDF says only changed, the list of dates marks its rows", async () => {
  const head = {
    court: "In the Court of the Civil Judge",
    caseLine: "Suit No. 1 of 2027",
    parties: [
      { name: "Acme Ltd", role: "Plaintiff" },
      { name: "A. Person", role: "Defendant" },
    ],
    title: "Written statement",
  };
  const side = {
    head: "As it was at 3:42 pm on 9 March",
    facts: "2 pages, 40 KB",
    saved: "Saved 9 March",
  };
  const onOpen = vi.fn();
  await show(
    <>
      <DraftSheet
        head={head}
        diffs={diffParagraphs(DRAFT_REVS["P1"] ?? [], DRAFT_REVS["P4"] ?? [])}
        marked
      />
      <PdfSheet
        title="Junior's note"
        changed
        state="Changed since 3:42 pm on 9 March"
        then={side}
        now={{ ...side, head: "As it is now" }}
        onOpen={onOpen}
      />
      <DatesSheet
        title="List of dates"
        rows={diffRows(ROWS, DATES_REVS["D1"] ?? {}, DATES_REVS["D3"] ?? {})}
        marked
      />
    </>,
  );
  expect(document.querySelectorAll(".page .ver-ins").length).toBeGreaterThan(3);
  expect(one(".ver-ins .ver-mine").textContent).toContain("Annexures P-2 and P-3");
  expect(one(".page-versus").textContent).toBe("Versus");
  expect(document.querySelectorAll(".page-paras [data-change]").length).toBe(5);
  expect(one(".ver-pdf-state").textContent).toBe("Changed since 3:42 pm on 9 March");
  expect(document.querySelectorAll(".ver-pdf-side")).toHaveLength(2);
  await click(button("Open now"));
  expect(onOpen).toHaveBeenCalledWith("now");
  expect(document.querySelectorAll(".ver-dates [data-change]").length).toBe(3);
});

const MATTER: MatterData = {
  papers: [
    { id: "plaint", title: "Plaint", pb: "PB 1 to 6", kind: "pleading", role: null },
    {
      id: "notice",
      title: "Notice to the bank, 7 July 2026",
      pb: "PB 7",
      kind: "correspondence",
      role: "supports",
    },
    { id: "reply", title: "Reply from the bank", pb: "PB 8", kind: "correspondence", role: null },
    { id: "letter", title: "Letter from the bank", pb: "PB 9", kind: "correspondence", role: null },
    { id: "receipt", title: "Postal receipt", pb: "PB 10", kind: "document", role: null },
    { id: "memo", title: "Junior's note", pb: "Note", kind: "note", role: null },
  ],
  cites: {},
};
const OWN = [
  { id: "dates", title: "List of dates", state: "accepted", word: "Dates" },
  { id: "draft", title: "Draft reply", state: "prepared", word: "Draft" },
] as const;

test("the wall as it stood: papers not there yet leave it, and the papers the version changed come forward", async () => {
  await show(
    <MatterContext value={MATTER}>
      <VersionWall version={at("v4")} looking own={OWN} />
    </MatterContext>,
  );
  expect(one('.tile[data-era="gone"]').querySelector(".tile-title")?.textContent).toContain(
    "Letter",
  );
  expect(one('.tile[data-era="touched"]').querySelector(".tile-title")?.textContent).toContain(
    "Draft reply",
  );
  // five of the matter's papers stood then
  expect(one(".ver-count").textContent).toBe("5");
});

test("a new draft saved from an earlier version stands beside the current draft, only at now", async () => {
  await show(
    <MatterContext value={MATTER}>
      <VersionWall version={at("v9")} looking={false} own={OWN} copy="Draft reply, 9 March" />
    </MatterContext>,
  );
  expect(one('.tile[id$="draft-copy"]').textContent).toContain("Draft reply, 9 March");
  expect(document.querySelector("[data-era]")).toBeNull();
});

test("where the history lives: on this computer, never in a folder that syncs", async () => {
  await show(
    <VersionWhere
      platform="mac"
      folder="~/Library/Application Support/clauseo"
      size="12 MB"
      settings="#settings"
      open
      onOpenChange={() => {}}
    />,
  );
  expect(words()).toContain("Never in OneDrive, Dropbox or any folder that syncs");
  expect(words()).toContain("~/Library/Application Support/clauseo");
  expect(words()).toContain("This matter's versions take 12 MB.");
});
