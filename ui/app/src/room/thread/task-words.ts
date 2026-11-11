// A task (an `execute` tool call) as the lawyer reads it (D-017, D-016): the agent's own plain words for it when it
// gave some (Claude's Bash `description`, Codex's `justification` or `reason`), else the program it ran and the paper
// it ran on ("python3", "Legal notice.pdf"); the command itself, its paths cut to the matter (D-046), for the fold
// under the row or the prompt; and for a task that failed, the last line of what went wrong. Pure: the matter's
// folder comes in as the host shows it (`folder_display`, below the home folder), and is only ever matched as text.
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { toolNameOf } from "./tool-names.ts";

export type Task = {
  /** The agent's own short words for the task, or null. */
  own: string | null;
  /** The program it ran ("python3"), or null. */
  program: string | null;
  /** The file name of the paper it ran on, or null. */
  paper: string | null;
  /** The whole command, its paths within the matter cut to the matter, or null with none. */
  command: string | null;
};

/** Past this, raw input or output isn't parsed: a long write's whole text is no place to look. */
const RAW_MAX = 16_384;
/** The most characters of the agent's own words, and of a failure's line. */
const SHORT = 120;
/** The shells whose `-c` / `-lc` script is the command itself. */
const SHELLS: ReadonlySet<string> = new Set([
  "bash",
  "sh",
  "zsh",
  "dash",
  "fish",
  "pwsh",
  "powershell",
]);
/** The extensions a paper of the matter has, so a script ("run.py") is never named as what was run on. */
const PAPER = /\.(pdf|docx?|rtf|odt|txt|md|xlsx?|csv|pptx?|eml|msg|html?|jpe?g|png|tiff?)$/i;
/** Words that start a command but say nothing of what it ran. */
const PREFIX: ReadonlySet<string> = new Set(["sudo", "env", "time", "nice", "exec", "command"]);

const short = (s: string): string => {
  const t = s.trim().replace(/\s+/g, " ");
  return t.length > SHORT ? `${t.slice(0, SHORT - 1)}…` : t;
};

function parsed(text: string | undefined): unknown {
  if (text === undefined || text.length > RAW_MAX) return undefined;
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
}

const objectOf = (v: unknown): Record<string, unknown> | null =>
  typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : null;

const text = (v: unknown): string | null =>
  typeof v === "string" && v.trim() !== "" ? v.trim() : null;

const baseName = (p: string): string =>
  p
    .replace(/[/\\]+$/, "")
    .split(/[/\\]/)
    .pop() ?? p;

/** A command given as an argument list ("bash", "-lc", "…"), as one line: a shell's script as it is, else the
 * arguments joined, one with a space in it quoted. */
function commandLine(v: unknown): string | null {
  if (typeof v === "string") return text(v);
  if (!Array.isArray(v) || !v.every((a) => typeof a === "string")) return null;
  const args = v as string[];
  if (args.length >= 3 && SHELLS.has(baseName(args[0]!)) && /^-l?c$/.test(args[1]!))
    return text(args.slice(2).join(" "));
  return text(args.map((a) => (/\s/.test(a) ? `'${a}'` : a)).join(" "));
}

/** The command's words, split as a shell would split them (quotes kept together, without the quotes). */
function words(command: string): string[] {
  const out: string[] = [];
  for (const m of command.matchAll(/"([^"]*)"|'([^']*)'|(\S+)/g)) out.push(m[1] ?? m[2] ?? m[3]!);
  return out;
}

/** The program a command runs: the first word of its first part that isn't a `cd`, a variable set, or a prefix. */
function programOf(command: string): string | null {
  for (const part of command.split(/&&|\|\||;|\|/)) {
    const ws = words(part.trim()).filter((w) => !/^[A-Za-z_][A-Za-z0-9_]*=/.test(w));
    while (ws.length > 0 && PREFIX.has(ws[0]!)) ws.shift();
    const first = ws[0];
    if (first === undefined || first === "cd" || first === "set") continue;
    const name = baseName(first);
    return name === "" ? null : name;
  }
  return null;
}

/** The first paper a command names, by its file name: a quoted or bare word ending in a paper's extension,
 * searched in quoted runs too, so a script's own strings (`open("…/Legal notice.pdf")`) count. */
function paperOf(command: string): string | null {
  for (const m of command.matchAll(/"([^"\n]+)"|'([^'\n]+)'|(\S+)/g)) {
    const w = (m[1] ?? m[2] ?? m[3]!).replace(/[),;:]+$/, "");
    if (PAPER.test(w)) return baseName(w);
    // a word inside a quoted script: its own runs
    const inner = /[\w./\\~-][^"'`]*?\.(pdf|docx?|rtf|odt|txt|xlsx?|csv|pptx?|eml|msg)\b/i.exec(w);
    if (inner !== null) return baseName((inner[0].split(/[("'`=]/).pop() ?? inner[0]).trim());
  }
  return null;
}

/** `s` with every path that runs through `root` cut to what follows it: from the path's start (a space or a quote
 * before it) through the root and its separator, where what leads up to the root ends in a separator (or is nothing,
 * or `~`), so "/Users/x/Documents/Sharma/a.pdf" with the root "Documents/Sharma" reads "a.pdf". */
function cutAt(s: string, root: string): string {
  let out = "";
  let i = 0;
  for (;;) {
    const hits = [s.indexOf(`${root}/`, i), s.indexOf(`${root}\\`, i)].filter((k) => k >= 0);
    if (hits.length === 0) break;
    const at = Math.min(...hits);
    const after = at + root.length + 1;
    let start = at;
    while (start > i && !/[\s"'`=(]/.test(s[start - 1]!)) start -= 1;
    const before = s.slice(start, at);
    out +=
      before === "" || before === "~" || /[/\\]$/.test(before)
        ? s.slice(i, start)
        : s.slice(i, after);
    i = after;
  }
  return out + s.slice(i);
}

/** Text with each path inside the matter's folder cut to the matter (D-046): `folder` is the host's display form,
 * below the home folder ("Documents/Clauseo/Sharma", no `~`) or the whole path elsewhere, matched only as text;
 * `cwd`, the folder the agent ran in, is cut the same way. */
export function inMatter(s: string, folder: string | null, cwd: string | null = null): string {
  let out = s;
  const f = folder?.replace(/[/\\]+$/, "") ?? "";
  if (f !== "") out = cutAt(out, f);
  const c = cwd?.replace(/[/\\]+$/, "") ?? "";
  if (c.length > 1) out = cutAt(out, c);
  return out;
}

/** A task's words and its command; every part null when the call holds none. */
export function taskOf(entry: ToolCallEntry, folder: string | null = null): Task {
  const raw = objectOf(parsed(entry.raw_input?.inline));
  const own =
    text(raw?.["description"]) ?? text(raw?.["justification"]) ?? text(raw?.["reason"]) ?? null;
  // the command from the raw input, else the title an agent gives a task (Codex's is its command; Claude's, in
  // backticks)
  const given = commandLine(raw?.["command"] ?? raw?.["cmd"]);
  const titled = entry.title.trim().replace(/^`+|`+$/g, "");
  // a title is taken for a command only when it reads as one, its first word a program's ("python3", "/bin/ls"):
  // an agent's own sentence ("Run ls papers") stays the title, and a tool's name ("exec_command") is no command
  const looks =
    entry.kind === "execute" &&
    /^[a-z0-9_.~/\\-]+(?:\s|$)/.test(titled) &&
    toolNameOf(titled) === undefined;
  const command = given ?? (looks ? titled : null);
  const cwd = text(raw?.["cwd"]) ?? text(raw?.["workdir"]);
  return {
    own: own === null ? null : short(own),
    program: command === null ? null : programOf(command),
    paper: command === null ? null : paperOf(command),
    command: command === null ? null : inMatter(command, folder, cwd),
  };
}

/** The last line of what went wrong in a failed call's output: its stderr, its error, or its text's last line; null
 * when it didn't fail, or its output says nothing (or is too large to read here). */
export function whyOf(entry: ToolCallEntry, folder: string | null = null): string | null {
  if (entry.status !== "failed") return null;
  const v = parsed(entry.raw_output?.inline);
  const raw = entry.raw_output?.inline;
  const o = objectOf(v);
  const said =
    text(o?.["stderr"]) ??
    text(o?.["error"]) ??
    text(objectOf(o?.["error"])?.["message"]) ??
    text(o?.["formatted_output"]) ??
    text(o?.["aggregated_output"]) ??
    text(o?.["output"]) ??
    (typeof v === "string" ? text(v) : null) ??
    (v === undefined && raw !== undefined && raw.length <= RAW_MAX ? text(raw) : null);
  if (said === null) return null;
  const lines = said.split(/\r?\n/).filter((l) => l.trim() !== "");
  const last = lines[lines.length - 1];
  return last === undefined ? null : short(inMatter(last, folder));
}
