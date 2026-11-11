// @vitest-environment happy-dom
// A file link in an answer (the agent's own markdown) opens the paper on the wall it names, as the wall's own tile
// does, whatever form the agent wrote it in: the matter's absolute path, a file:// URL, ChatGPT's sandbox: path, a
// relative path or a bare name. A file anywhere else, Clauseo's own folders included, is the link's text alone, never
// a link that does nothing; a web link still goes to the host's open_external, which confirms it.
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { agentText, reset, userMessage, uuid } from "../testing/stream-script.ts";
import type { PaperSummary } from "../transport/protocol.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import { mountRoom, summary, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

const REPLY = uuid(17, 8);
const NOTICE = uuid(18, 8);
const NOTES = uuid(19, 8);
const BRIEF_A = uuid(20, 8);
const BRIEF_B = uuid(21, 8);
const FOLDER = "Documents/Clauseo/Sharma v. State of Delhi";
const MATTER_PATH = `/Users/rohan/${FOLDER}`;
const ENCODED = `/Users/rohan/${FOLDER.replaceAll(" ", "%20")}`;

const wall = (): PaperSummary[] => [
  { file: REPLY, path: "Reply to legal notice.docx", size: 1, modified: 0 },
  { file: NOTICE, path: "drafts/Notice.docx", size: 1, modified: 0 },
  { file: BRIEF_A, path: "a/Brief.docx", size: 1, modified: 0 },
  { file: BRIEF_B, path: "b/Brief.docx", size: 1, modified: 0 },
];

async function answer(text: string, papers = wall(), folder = FOLDER): Promise<Harness> {
  const room = await mountRoom({
    next: fakeNext({
      matters: () => [detailed(THE_MATTER, { folder_display: folder })],
      threads: () => [{ ...summary(), seen: true }],
      papers,
    }),
  });
  room.deliver([reset([userMessage(1, 1, "Draft the reply"), agentText(2, 1, text)])]);
  await room.settle();
  return room;
}

const answerLinks = () => [
  ...(h?.host.querySelectorAll<HTMLElement>(".reply-text a[data-clauseo='link']") ?? []),
];
const linkNamed = (text: string) => answerLinks().find((a) => a.textContent === text);
const plainNamed = (text: string) =>
  [...(h?.host.querySelectorAll<HTMLElement>("[data-clauseo='link-text']") ?? [])].find(
    (s) => s.textContent === text,
  );
const opened = () => h?.transport.only("open_file").map((c) => c.file) ?? [];
const external = () => h?.transport.only("open_external").map((c) => c.url) ?? [];
const enter = async (el: HTMLElement | undefined) => {
  if (el === undefined) throw new Error("no link");
  el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  await h?.settle();
};

test("every form an agent writes a matter paper's link in opens it with the wall's own open_file, by click or Enter", async () => {
  const forms = [
    // what the lawyer's answer said: an absolute path in the matter, percent-encoded
    `[Download the draft reply to the legal notice.](${ENCODED}/Reply%20to%20legal%20notice.docx)`,
    `[spaces](<${MATTER_PATH}/Reply to legal notice.docx>)`,
    `[file url](file://${ENCODED}/Reply%20to%20legal%20notice.docx)`,
    "[sandbox](<sandbox:/mnt/data/Reply to legal notice.docx>)",
    "[sandbox encoded](sandbox:/mnt/data/Reply%20to%20legal%20notice.docx)",
    "[bare](<Reply to legal notice.docx>)",
    "[dot](./Reply%20to%20legal%20notice.docx)",
    `[tilde](~/${FOLDER.replaceAll(" ", "%20")}/Reply%20to%20legal%20notice.docx)`,
    "[relative](drafts/Notice.docx)",
  ];
  h = await answer(forms.join("\n\n"));
  const names = answerLinks().map((a) => a.textContent);
  expect(names).toEqual([
    "Download the draft reply to the legal notice.",
    "spaces",
    "file url",
    "sandbox",
    "sandbox encoded",
    "bare",
    "dot",
    "tilde",
    "relative",
  ]);
  for (const a of answerLinks()) {
    // the same link look, reached by the keyboard, never an href to navigate
    expect(a.getAttribute("role")).toBe("link");
    expect(a.tabIndex).toBe(0);
    expect(a.hasAttribute("href")).toBe(false);
    await h.click(a);
    await h.settle();
  }
  expect(opened()).toEqual([...Array<string>(8).fill(REPLY), NOTICE]);
  await enter(linkNamed("Download the draft reply to the legal notice."));
  expect(opened().at(-1)).toBe(REPLY);
  expect(opened()).toHaveLength(10);
  // the wall's tile for the same paper sends the same command with the same file ref
  const tile = [...h.host.querySelectorAll("aside.room-pane li.tile")].find(
    (t) => t.querySelector(".tile-title")?.textContent === "Reply to legal notice.docx",
  );
  await h.click(tile?.querySelector<HTMLButtonElement>("button.tile-face"));
  await h.settle();
  expect(h.transport.only("open_file").at(-1)).toEqual({ call: "open_file", file: REPLY });
  expect(external()).toEqual([]);
});

test("a file outside the matter, a .. out of it, a shared name and a paper not on the wall are the link's text alone", async () => {
  const forms = [
    "[run folder](</Users/rohan/Library/Application Support/ai.clauseo.desktop/agents/codex/home/Reply to legal notice.docx>)",
    "[work folder](file:///Users/rohan/Library/Caches/ai.clauseo.desktop/run/Reply%20to%20legal%20notice.docx)",
    "[passwd](/etc/passwd)",
    "[outside](../outside.docx)",
    "[ambiguous](Brief.docx)",
    "[not on the wall](<Reply notes.md>)",
    "[another scheme](ftp://example.com/x.docx)",
    "[custom scheme](vscode://file/Users/rohan/Reply.docx)",
  ];
  h = await answer(`See ${forms.join(", ")}.`);
  expect(answerLinks()).toEqual([]);
  for (const name of [
    "run folder",
    "work folder",
    "passwd",
    "outside",
    "ambiguous",
    "not on the wall",
    "another scheme",
    "custom scheme",
  ]) {
    const plain = plainNamed(name);
    expect(plain?.tagName).toBe("SPAN");
    expect(plain?.getAttribute("role")).toBeNull();
    expect(plain?.hasAttribute("tabindex")).toBe(false);
    await h.click(plain);
  }
  await h.settle();
  expect(opened()).toEqual([]);
  expect(external()).toEqual([]);
  expect(h.one("error")).toBeNull();
});

test("a link to a paper that reaches the wall later becomes a link; one that leaves the wall becomes text", async () => {
  const papers = wall();
  h = await answer(
    "Made [Reply notes.md](Reply%20notes.md) and [the notice](drafts/Notice.docx).",
    papers,
  );
  expect(linkNamed("Reply notes.md")).toBeUndefined();
  expect(plainNamed("Reply notes.md")).toBeDefined();
  papers.splice(1, 1);
  // the notes arrive and the wall hears the folder change: the text becomes a link that opens them
  papers.push({ file: NOTES, path: "Reply notes.md", size: 1, modified: 0 });
  await act(async () => h?.transport.appEvent({ papers_changed: { matter: THE_MATTER.matter } }));
  await h.settle();
  await h.settle();
  expect(plainNamed("Reply notes.md")).toBeUndefined();
  await h.click(linkNamed("Reply notes.md"));
  await h.settle();
  expect(opened()).toEqual([NOTES]);
  // and the notice, gone from the wall, is now its text alone
  expect(linkNamed("the notice")).toBeUndefined();
  expect(plainNamed("the notice")).toBeDefined();
});

test("a web link still goes to the host's open_external, and a deceptive one still shows its real host for the confirm", async () => {
  h = await answer(
    "Read [the act](https://indiacode.nic.in/act), [Visit sbi.co.in](https://evil.example.net/login) and [mail](mailto:clerk@court.in).",
  );
  const statute = linkNamed("the act");
  expect(statute?.getAttribute("href")).toBe("https://indiacode.nic.in/act");
  await h.click(statute);
  const deceptive = answerLinks().find((a) => a.textContent?.startsWith("Visit sbi.co.in"));
  expect(deceptive?.querySelector("[data-clauseo='link-host']")?.textContent).toBe(
    " (evil.example.net)",
  );
  await h.click(deceptive);
  await h.click(linkNamed("mail"));
  await h.settle();
  expect(external()).toEqual([
    "https://indiacode.nic.in/act",
    "https://evil.example.net/login",
    "mailto:clerk@court.in",
  ]);
  expect(opened()).toEqual([]);
});

test("the forms ChatGPT wrote on Rohan's MacBook open their papers: angle brackets, spaces, parentheses, a line suffix", async () => {
  const MSA = "Master Services Agreement - Acme and Kestrel - draft v2 (Kestrel comments).docx";
  const papers: PaperSummary[] = [
    { file: REPLY, path: "Reply to legal notice.docx", size: 1, modified: 0 },
    { file: uuid(30, 8), path: MSA, size: 1, modified: 0 },
    { file: uuid(31, 8), path: "Bank statement (March).pdf", size: 1, modified: 0 },
    { file: uuid(32, 8), path: "Ledger - FY 2025 (draft).csv", size: 1, modified: 0 },
    { file: uuid(33, 8), path: "My Project/My Report.md", size: 1, modified: 0 },
  ];
  const root = "/Users/rohan/Documents/tmatter";
  h = await answer(
    [
      `[Open the reply](<${root}/Reply to legal notice.docx>)`,
      `[Open the agreement](<${root}/${MSA}>)`,
      `[Open the statement](<${root}/Bank statement (March).pdf>)`,
      `[Open the ledger](<${root}/Ledger - FY 2025 (draft).csv>)`,
      `[Open the report](<${root}/My Project/My Report.md:3>)`,
      `[Report at a line and column](<${root}/My Project/My Report.md:3:7>)`,
      `[Report at a GitHub line](<${root}/My Project/My Report.md#L12-L20>)`,
      "[outside, at a line](/Users/rohan/code/app.py:12)",
      `[not on the wall, at a line](<${root}/Reply notes.md:12>)`,
      "[a bare name at a line](<Reply notes.md:12>)",
    ].join("\n\n"),
    papers,
    "Documents/tmatter",
  );
  const opens = [
    "Open the reply",
    "Open the agreement",
    "Open the statement",
    "Open the ledger",
    "Open the report",
    "Report at a line and column",
    "Report at a GitHub line",
  ];
  expect(answerLinks().map((a) => a.textContent)).toEqual(opens);
  for (const name of opens) await h.click(linkNamed(name));
  await h.settle();
  const files = papers.map((p) => p.file);
  expect(opened()).toEqual([...files, files[4], files[4]]);
  for (const name of opens) await enter(linkNamed(name));
  expect(opened()).toEqual([...files, files[4], files[4], ...files, files[4], files[4]]);
  for (const name of ["outside, at a line", "not on the wall, at a line", "a bare name at a line"])
    expect(plainNamed(name)?.tagName).toBe("SPAN");
});
