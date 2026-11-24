// A task's plain words, its command cut to the matter and a failure's one line (D-016, D-017, D-046); a web search's
// site said apart (D-028); Markdown marks taken off a title or thought (D-018).
import { expect, test } from "vitest";
import { toolCall } from "../../testing/stream-script.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { doneOf, timeOf } from "./steps.ts";
import { doingOf, plainOf, siteQuery, thoughtText, titleOf } from "./step-names.ts";
import { mainPartsOf } from "../../testing/main-parts.ts";
import { inMatter, programsIn, programsOf, taskOf, whyOf } from "./task-words.ts";

const FOLDER = "Documents/Clauseo/Sharma";
const ROOT = `/Users/lawyer/${FOLDER}`;

const task = (raw: object | null, over: Partial<ToolCallEntry> = {}): ToolCallEntry => ({
  ...toolCall(7, 1, "completed").tool_call!,
  kind: "execute",
  title: "",
  ...(raw === null ? {} : { raw_input: { inline: JSON.stringify(raw) } }),
  ...over,
});

test("Claude's own description leads, its command kept for the fold", () => {
  const t = taskOf(task({ command: `ls ${ROOT}/papers`, description: "List the papers" }), FOLDER);
  expect(t.own).toBe("List the papers");
  expect(t.command).toBe("ls papers");
  expect(doneOf("en", task({ command: "ls", description: "List the papers" }))).toBe(
    "Ran: list the papers.",
  );
});

test("Codex's justification leads; with none, the program and the paper it ran on", () => {
  expect(taskOf(task({ command: ["ls"], justification: "See what is filed" })).own).toBe(
    "See what is filed",
  );
  const raw = {
    command: ["/bin/zsh", "-lc", `python3 -c "print(open('${ROOT}/papers/Legal notice.pdf'))"`],
    cwd: ROOT,
  };
  const t = taskOf(task(raw), FOLDER);
  expect(t).toMatchObject({ own: null, program: "python3", paper: "Legal notice.pdf" });
  expect(t.command).toBe(`python3 -c "print(open('papers/Legal notice.pdf'))"`);
  expect(doneOf("en", task(raw))).toBe("Ran python3 on Legal notice.pdf.");
  expect(doneOf("en", task(raw, { status: "failed" }))).toBe(
    "Tried to run python3 on Legal notice.pdf.",
  );
  expect(doingOf("en", task(raw, { status: "in_progress" }))).toBe(
    "Running python3 on Legal notice.pdf.",
  );
  expect(doneOf("en", task({ command: "cd papers && wc -l notes.txt" }))).toBe(
    "Ran wc on notes.txt.",
  );
  expect(doneOf("en", task({ command: "swift run" }))).toBe("Ran swift.");
});

test("an escape inside a script's string is no part of a paper's name", () => {
  const notes = task({
    command: `python3 -c "open('notes.md','w').write('## Findings\\nNotice-E1.docx read in full')"`,
  });
  expect(doneOf("en", notes)).not.toContain("nNotice-E1.docx");
  expect(doneOf("en", notes)).toBe("Ran python3.");
  expect(taskOf(task({ command: "python3 -c \"print('a\\tLease.pdf')\"" })).paper).toBeNull();
  // a real paper on the command line is still named, a Windows path's backslashes still its folders
  expect(doneOf("en", task({ command: 'python3 script.py "Notice-E1.docx"' }))).toBe(
    "Ran python3 on Notice-E1.docx.",
  );
  expect(taskOf(task({ command: 'python3 x.py "C:\\matter\\notes\\new.docx"' })).paper).toBe(
    "new.docx",
  );
});

test("a title is the command only when it reads as one; with nothing, a task as before", () => {
  expect(taskOf(task(null, { title: "`python3 run.py`" })).program).toBe("python3");
  expect(taskOf(task(null, { title: "Run ls papers" }))).toMatchObject({
    program: null,
    command: null,
  });
  expect(doneOf("en", task(null))).toBe("Ran a command.");
});

test("a chained command is counted, not named by its first program; its command kept whole for the fold", () => {
  const ps = {
    command: [
      "powershell",
      "-c",
      "New-Item -ItemType Directory drafts; Copy-Item notice.docx drafts | Out-Null",
    ],
  };
  const t = taskOf(task(ps));
  expect(t).toMatchObject({ program: "New-Item", count: 3 });
  expect(t.command).toBe(
    "New-Item -ItemType Directory drafts; Copy-Item notice.docx drafts | Out-Null",
  );
  expect(doneOf("en", task(ps))).toBe("Ran 3 commands.");
  expect(doneOf("en", task(ps, { status: "failed" }))).toBe("Tried to run 3 commands.");
  expect(doingOf("en", task(ps, { status: "in_progress" }))).toBe("Running 3 tasks.");
  // hi: later, so the English
  expect(doneOf("hi", task(ps))).toBe("Ran 3 commands.");
  expect(doneOf("hi", task(ps, { status: "failed" }))).toBe("Tried to run 3 commands.");
  expect(doingOf("hi", task(ps, { status: "in_progress" }))).toBe("3 काम चल रहे हैं।");
  expect(doneOf("en", task({ command: "mkdir out && pandoc a.md -o out/a.docx" }))).toBe(
    "Ran 2 commands.",
  );
  // a cd, a quoted or escaped separator and 2>&1 chain nothing: one program, named as before
  expect(doneOf("en", task({ command: "cd papers && wc -l notes.txt" }))).toBe(
    "Ran wc on notes.txt.",
  );
  expect(doneOf("en", task({ command: `python3 -c "a = 1; print(a)" 2>&1` }))).toBe("Ran python3.");
  expect(taskOf(task({ command: "find . -name '*.tmp' -exec rm {} \\;" })).count).toBe(1);
  expect(taskOf(task({ command: "find . -name x -exec rm {} \\;" })).count).toBe(1);
  // a backslash is a Windows path's: it escapes nothing
  expect(doneOf("en", task({ command: "Get-ChildItem C:\\m\\; Remove-Item papers" }))).toBe(
    "Ran 2 commands.",
  );
  expect(doneOf("en", task({ command: "cd C:\\matter\\; Remove-Item -Recurse papers" }))).toBe(
    "Ran Remove-Item.",
  );
  expect(taskOf(task({ command: "grep -c 'a|b' notes.txt || true" })).count).toBe(2);
  // the agent's own words still lead, after "Ran:"
  expect(doneOf("en", task({ ...ps, description: "Copy the notice into drafts" }))).toBe(
    "Ran: copy the notice into drafts.",
  );
});

test.each([
  ["echo 'it'\\''s' ; rm -rf papers", ["echo", "rm"]],
  ['echo "a \\" b" ; rm -rf papers', ["echo", "rm"]],
  ['echo `"; Remove-Item papers', ["echo", "Remove-Item"]],
  ["Write-Output \\; Remove-Item papers", ["Write-Output", "Remove-Item"]],
  ['echo "abc ; rm -rf papers', ["echo", "rm"]],
  ["echo 'abc ; rm -rf papers", ["echo", "rm"]],
  // a path's backslash before a quote or a semicolon, in PowerShell
  ['echo C:\\m\\"x" ; Remove-Item papers ; echo "y"', ["echo", "Remove-Item", "echo"]],
  ["Write-Output -ok C:\\m\\; Remove-Item papers", ["Write-Output", "Remove-Item"]],
  ["find . -exec x {}\\; Remove-Item papers", ["find", "Remove-Item"]],
  // sh's escaped quote inside double quotes, and PowerShell's backtick
  ['echo "x\\"" ; rm -rf papers ; echo "y"', ["echo", "rm", "echo"]],
  ['echo `" ; rm x ; echo "y"', ["echo", "rm", "echo"]],
])("no quote or backslash hides a chained task: %s", (line, programs) => {
  expect(programsOf(line)).toEqual(programs);
  const n = String(programs.length);
  expect(taskOf(task({ command: line }))).toMatchObject({
    program: programs[0],
    count: programs.length,
  });
  expect(doneOf("en", task({ command: line }))).toBe(`Ran ${n} commands.`);
});

test("find's \\; after an -exec is one task, in every reading", () => {
  expect(programsOf("find . -exec rm {} \\;")).toEqual(["find"]);
  expect(programsOf("find . -name x -exec rm {} \\;")).toEqual(["find"]);
  expect(programsOf("find . -name x -execdir rm {} \\; && ls")).toEqual(["find", "ls"]);
});

/** A seeded PRNG (mulberry32), so a failing line comes back on every run. */
function prng(seed: number): () => number {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const PIECES = [
  "echo",
  "rm",
  "Remove-Item",
  "ls",
  "find",
  "x",
  "papers",
  "'",
  '"',
  "`",
  "\\",
  ";",
  "|",
  "&&",
  " ",
  " ",
  "C:\\m\\",
  "C:\\matter\\x.txt",
  "-exec rm {} \\;",
  "-ok",
  "{}\\;",
  "''",
  "2>&1",
  "\\;",
];

test("no line counts fewer tasks than main's reading did (2000 seeded lines)", () => {
  const next = prng(1110);
  let failing: string | null = null;
  for (let n = 0; n < 2000 && failing === null; n++) {
    const length = 1 + Math.floor(next() * 14);
    let line = "";
    for (let k = 0; k < length; k++) line += PIECES[Math.floor(next() * PIECES.length)]!;
    if (programsOf(line).length < programsIn(mainPartsOf(line)).length) failing = line;
  }
  expect(failing, `first line counted below main: ${failing}`).toBeNull();
});

test("paths inside the matter read from the matter, through the home folder or a whole path", () => {
  expect(inMatter(`cat "${ROOT}/papers/a b.pdf" ~/${FOLDER}/x.txt`, FOLDER)).toBe(
    `cat "papers/a b.pdf" x.txt`,
  );
  expect(inMatter("cat /Users/lawyer/Other/Sharma/x.txt", FOLDER)).toBe(
    "cat /Users/lawyer/Other/Sharma/x.txt",
  );
  expect(inMatter("type D:\\Matters\\Acme\\a.pdf", "D:\\Matters\\Acme")).toBe("type a.pdf");
});

test("a failed task says why in one line: its stderr, its error, or its output's last line", () => {
  const failed = (out: string) =>
    task({ command: "x" }, { status: "failed", raw_output: { inline: out } });
  expect(whyOf(failed(JSON.stringify({ stderr: "warn\nError: no such file\n" })))).toBe(
    "Error: no such file",
  );
  expect(whyOf(failed(JSON.stringify({ result: null, error: "permission denied" })))).toBe(
    "permission denied",
  );
  expect(whyOf(failed(`Traceback\n  File "${ROOT}/a.py"\nValueError: bad`), FOLDER)).toBe(
    "ValueError: bad",
  );
  expect(whyOf(task({ command: "x" }, { raw_output: { inline: '"ok"' } }))).toBeNull();
});

test("a search of one site names the site apart from its words", () => {
  expect(siteQuery("site:indiacode.nic.in Negotiable Instruments Act")).toEqual({
    site: "indiacode.nic.in",
    query: "Negotiable Instruments Act",
  });
  expect(siteQuery('"notice period" rent')).toEqual({ site: null, query: "notice period rent" });
  const fetch = task({ query: "site:indiacode.nic.in section 138" }, { kind: "fetch" });
  expect(doneOf("en", fetch)).toBe("Searched indiacode.nic.in for 'section 138'.");
});

test("Markdown's marks come off a title or a thought", () => {
  expect(plainOf("**Inspecting docx headers**")).toBe("Inspecting docx headers");
  expect(plainOf("## Plan\nRead *the* notice and `grep` it")).toBe(
    "Plan\nRead the notice and grep it",
  );
  expect(plainOf("2 * 3 * 4")).toBe("2 * 3 * 4");
  expect(titleOf("**Read** /Users/lawyer/x/Lease.pdf")).toBe("Read Lease.pdf");
});

test("a row's time is when it started, whenever it finished (D-020)", () => {
  expect(timeOf({ ...task(null), started_at: 1000, finished_at: 9000 })).toBe(1000);
  expect(timeOf({ ...task(null, { status: "in_progress" }), started_at: 1000 })).toBeNull();
});

test("a thought keeps no blank line before or after it, nor more than one in a row (D-030)", () => {
  expect(thoughtText("\n\n**India Code link unverified**\n\n\n\nChecking it.\n")).toBe(
    "India Code link unverified\n\nChecking it.",
  );
  expect(thoughtText(" \n\n ")).toBe("");
});
