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
  /** The program it ran ("python3"), or null: the first of a chained command's. */
  program: string | null;
  /** How many programs the command runs, chained with `&&`, `||`, `;` or `|` (a `cd` or a `set` not counted):
   * past one, the row counts them ("Ran 2 tasks.") rather than name only the first; 0 with no command. */
  count: number;
  /** The file name of the paper it ran on, or null. */
  paper: string | null;
  /** The whole command, its paths within the matter cut to the matter, or null with none. */
  command: string | null;
  /** What the task does, from the call's own title or the text it shows, ready to follow "Ran" ("the checks",
   * "convert the MSA to Markdown"); null with neither, or when the agent's words or the command say it. */
  described: string | null;
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
/** Programs that only move to a folder or set a variable: never what a command ran, never counted. */
const PLACE: ReadonlySet<string> = new Set(["cd", "set", "pushd", "popd", "Set-Location"]);

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

/** A command line's parts, chained with `&&`, `||`, `;` or `|`: the line read three ways, as the
 * reading that finds the most tasks has it. One reading is main's (mainPartsOf), so a count is never lower than it
 * was; the others are sh's (shellPartsOf) and PowerShell's. Each is one pass over the line, plus one re-read from
 * each quote that never closes. */
function readingsOf(command: string): string[][] {
  return [mainPartsOf(command), shellPartsOf(command, "sh"), shellPartsOf(command, "pwsh")];
}

/** Main's reading, as it was before the sh and PowerShell readings: split outside quotes, never at `2>&1`'s lone
 * `&`; a backslash escapes nothing but find's `\;` as a word of its own. Kept as it is, so no line counts
 * fewer tasks than it did. */
function mainPartsOf(command: string): string[] {
  const parts: string[] = [];
  let quote: string | null = null;
  let from = 0;
  for (let i = 0; i < command.length; i++) {
    const c = command[i]!;
    if (quote !== null) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === "\\" && command[i + 1] === ";" && (i === 0 || /\s/.test(command[i - 1]!)))
      i += 1;
    else if (c === ";" || c === "|" || (c === "&" && command[i + 1] === "&")) {
      parts.push(command.slice(from, i));
      if (c !== ";" && command[i + 1] === c) i += 1;
      from = i + 1;
    }
  }
  parts.push(command.slice(from));
  return parts;
}

/** A line's parts as sh or PowerShell splits it, outside quotes and never at `2>&1`'s lone `&`.
 * - sh: outside quotes a backslash escapes the next character; inside double quotes it escapes `"`, `\`, `$` and a
 *   backtick; inside single quotes nothing escapes.
 * - PowerShell: a backslash is a path's (`cd C:\matter\; ls` is two parts); a backtick escapes the next character
 *   outside quotes and inside double quotes; inside single quotes `''` is a quote.
 * In both, find's `\;` is no split when the backslash follows a space and an `-exec`, `-execdir`, `-ok` or `-okdir`
 * came earlier in the part. A quote that never closes is read again from where it stands as a plain character, once
 * per such quote, so no unclosed quote hides a part. */
function shellPartsOf(command: string, shell: "sh" | "pwsh"): string[] {
  const escape = shell === "sh" ? "\\" : "`";
  const parts: string[] = [];
  /** The quotes that never closed, read as plain characters. */
  const plain = new Set<number>();
  let quote: string | null = null;
  /** Where the open quote stands, where its part starts and how many parts came before it. */
  let opened = { at: -1, from: 0, parts: 0 };
  let from = 0;
  for (let i = 0; i <= command.length; i++) {
    if (i === command.length) {
      if (quote === null) break;
      // the quote never closed: from it again, as a plain character
      plain.add(opened.at);
      quote = null;
      from = opened.from;
      parts.length = opened.parts;
      i = opened.at - 1;
      continue;
    }
    const c = command[i]!;
    const next = command[i + 1];
    if (quote === "'") {
      if (c === "'") {
        if (shell === "pwsh" && next === "'") i += 1;
        else quote = null;
      }
    } else if (quote === '"') {
      if (c === escape && (shell === "pwsh" || (next !== undefined && '"\\$`'.includes(next))))
        i += 1;
      else if (c === '"') quote = null;
    } else if (c === "\\" && next === ";" && findsEnd(command, from, i)) i += 1;
    else if (c === escape) i += 1;
    else if ((c === '"' || c === "'") && !plain.has(i)) {
      quote = c;
      opened = { at: i, from, parts: parts.length };
    } else if (c === ";" || c === "|" || (c === "&" && next === "&")) {
      parts.push(command.slice(from, i));
      if (c !== ";" && next === c) i += 1;
      from = i + 1;
    }
  }
  parts.push(command.slice(from));
  return parts;
}

/** Whether the backslash at `at` ends a find's `-exec` (`find … -exec rm {} \;`): it follows a space, and an
 * `-exec`, `-execdir`, `-ok` or `-okdir` came earlier in the part that starts at `from`. */
function findsEnd(command: string, from: number, at: number): boolean {
  return (
    at > 0 &&
    /\s/.test(command[at - 1]!) &&
    /(?:^|\s)-(?:exec|execdir|ok|okdir)\s/.test(command.slice(from, at))
  );
}

/** The programs in a line's parts, one per part: each part's first word that isn't a variable set or a prefix, a
 * part that only moves to a folder or sets a variable (PLACE) left out. */
export function programsIn(parts: string[]): string[] {
  const out: string[] = [];
  for (const part of parts) {
    const ws = words(part.trim()).filter((w) => !/^[A-Za-z_][A-Za-z0-9_]*=/.test(w));
    while (ws.length > 0 && PREFIX.has(ws[0]!)) ws.shift();
    const first = ws[0];
    if (first === undefined || PLACE.has(first)) continue;
    const name = baseName(first);
    if (name !== "") out.push(name);
  }
  return out;
}

/** The programs a command runs, as the reading that finds the most of them has it (readingsOf); between readings that
 * find as many, the one with the most parts, then the first. */
export function programsOf(command: string): string[] {
  let best: { programs: string[]; parts: number } | null = null;
  for (const parts of readingsOf(command)) {
    const programs = programsIn(parts);
    if (
      best === null ||
      programs.length > best.programs.length ||
      (programs.length === best.programs.length && parts.length > best.parts)
    )
      best = { programs, parts: parts.length };
  }
  return best!.programs;
}

/** A string's escape for a new line, a tab or a return (`\n`, `\t`, `\r`): a lone backslash's, never a doubled
 * one's (`C:\\matter\\new.docx` in a script). */
const ESCAPE = /(?<!\\)(?:\\\\)*\\[ntr]/;
/** A Windows path, whose single backslashes are its folders' (`C:\matter\notes\new.docx`). */
const WINDOWS = /^(?:[A-Za-z]:\\|\\\\)/;
/** Whether a word holds a string's escape, so it is a script's text (`'## Findings\nNotice.docx read'`) and no path. */
const escaped = (w: string): boolean => !WINDOWS.test(w) && ESCAPE.test(w);

/** The first paper a command names, by its file name: a quoted or bare word ending in a paper's extension,
 * searched in quoted runs too, so a script's own strings (`open("…/Legal notice.pdf")`) count. A word that holds a
 * string's escape (escaped) is a script's text, never a paper. */
function paperOf(command: string): string | null {
  for (const m of command.matchAll(/"([^"\n]+)"|'([^'\n]+)'|(\S+)/g)) {
    const w = (m[1] ?? m[2] ?? m[3]!).replace(/[),;:]+$/, "");
    if (PAPER.test(w) && !escaped(w)) return baseName(w);
    // a word inside a quoted script: its own runs
    for (const inner of w.matchAll(
      /[\w./\\~-][^"'`]*?\.(pdf|docx?|rtf|odt|txt|xlsx?|csv|pptx?|eml|msg)\b/gi,
    )) {
      const run = (inner[0].split(/[("'`=]/).pop() ?? inner[0]).trim();
      if (!escaped(run)) return baseName(run);
    }
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
  const programs = command === null ? [] : programsOf(command);
  const program = programs[0] ?? null;
  return {
    own: own === null ? null : short(own),
    program,
    count: programs.length,
    paper: command === null ? null : paperOf(command),
    command: command === null ? null : inMatter(command, folder, cwd),
    described: own === null && program === null ? describedOf(entry, titled, looks) : null,
  };
}

/** A task's title in words ("Run the checks"), else the first line of the text it shows (Claude's description of a
 * command): its leading "Run" and closing stop off, in sentence case after "Ran", cut short. Never a tool's name. */
function describedOf(entry: ToolCallEntry, titled: string, looks: boolean): string | null {
  let shown: string | undefined;
  for (const c of entry.content)
    if (typeof c === "object" && c.text?.text.inline !== undefined) {
      shown = c.text.text.inline;
      break;
    }
  const from =
    titled !== "" && !looks && toolNameOf(titled) === undefined
      ? titled
      : (shown?.split(/\r?\n/).find((l: string) => l.trim() !== "") ?? "");
  const words = from
    .trim()
    .replace(/^(?:run|running|ran)\s+/i, "")
    .replace(/[.!?।…]+$/, "")
    .trim();
  if (words === "" || toolNameOf(words) !== undefined) return null;
  // sentence case: the first word lower, unless it is a name or an acronym ("MSA", "McKinsey", "Word")
  const first = words.split(/\s/)[0]!;
  const lower = /^[A-Z][a-z]+$/.test(first) && !PROPER.has(first) ? first.toLowerCase() : first;
  return short(lower + words.slice(first.length));
}

/** Names a task's words may start with that stay capitalised. */
const PROPER: ReadonlySet<string> = new Set([
  "Word",
  "Excel",
  "Clauseo",
  "Python",
  "Claude",
  "ChatGPT",
]);

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
