// A task's plain words, its command cut to the matter and a failure's one line (D-016, D-017, D-046); a web search's
// site said apart (D-028); Markdown marks taken off a title or thought (D-018).
import { expect, test } from "vitest";
import { toolCall } from "../../testing/stream-script.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { doneOf, timeOf } from "./steps.ts";
import { doingOf, plainOf, siteQuery, thoughtText, titleOf } from "./step-names.ts";
import { inMatter, taskOf, whyOf } from "./task-words.ts";

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
    "List the papers.",
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

test("a title is the command only when it reads as one; with nothing, a task as before", () => {
  expect(taskOf(task(null, { title: "`python3 run.py`" })).program).toBe("python3");
  expect(taskOf(task(null, { title: "Run ls papers" }))).toMatchObject({
    program: null,
    command: null,
  });
  expect(doneOf("en", task(null))).toBe("Ran a task.");
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
