// Steps shaped as the two agents send them, for the step lines that name what ran (issue 13): Claude's
// (claude-agent-acp: its tool's name in tool_name, an MCP call titled `mcp__<server>__<tool>` with kind other, a
// skill titled "Load skill: <ns:name>", Bash with the command as its title and its description in the input) and
// ChatGPT's (codex-acp 2.2.2: no tool name, an MCP call titled `mcp.<server>.<tool>` with kind execute and the input
// `{server, tool, arguments}`, a skill read as its SKILL.md, a command as its title with its argument list). Over the
// Sharma matter's papers. For the tests, the copy review and the shots.
import type { ToolCallEntry, ToolCallStatus, ToolKind, ViewEntry } from "../transport/protocol.ts";
import { toolCall, uuid } from "./stream-script.ts";
import type { NoticeEntry, NoticeKind } from "../transport/protocol.ts";

export type Agent = "Claude" | "ChatGPT";
export const AGENTS: readonly Agent[] = ["Claude", "ChatGPT"];

/** The matter's folder, as the agent names it. */
export const MATTER = "/Users/x/Matters/Sharma v. Union Bank";

let next = 0;
/** A call with its own id and order, in `status`. */
export function stepCall(
  kind: ToolKind,
  title: string,
  status: ToolCallStatus,
  more: Partial<ToolCallEntry> & { raw?: object } = {},
): ToolCallEntry {
  next += 1;
  const { raw, ...rest } = more;
  return {
    ...toolCall(10 + next, 700 + next, status).tool_call!,
    tool_call: uuid(700 + next, 7),
    order: 10 + next,
    kind,
    title,
    ...(raw === undefined ? {} : { raw_input: { inline: JSON.stringify(raw) } }),
    ...rest,
  };
}

/** A read of the matter's paper `name`. */
export function readOf(agent: Agent, name: string, status: ToolCallStatus = "completed") {
  const path = `${MATTER}/${name}`;
  return agent === "Claude"
    ? stepCall("read", `Read ${path}`, status, {
        tool_name: "Read",
        raw: { file_path: path },
        locations: [{ inside: { path } }],
      })
    : stepCall("read", `Read ${name}`, status, {
        raw: { command: ["bash", "-lc", `sed -n '1,200p' '${name}'`], cwd: MATTER },
        locations: [{ inside: { path } }],
      });
}

/** A search of the papers for `pattern`. */
export function searchOf(agent: Agent, pattern: string, status: ToolCallStatus = "completed") {
  return agent === "Claude"
    ? stepCall("search", `grep "${pattern}"`, status, { tool_name: "Grep", raw: { pattern } })
    : stepCall("search", `Search ${pattern}`, status, {
        raw: { command: ["bash", "-lc", `rg -n '${pattern}'`], cwd: MATTER, query: pattern },
      });
}

/** A skill loaded: Claude's Skill tool, or ChatGPT's read of the skill's SKILL.md. */
export function skillOf(agent: Agent, skill: string, status: ToolCallStatus = "completed") {
  const bare = skill.replace(/^.*:/, "");
  return agent === "Claude"
    ? stepCall("other", `Load skill: ${skill}`, status, { tool_name: "Skill", raw: { skill } })
    : stepCall("read", `Read SKILL.md`, status, {
        locations: [{ outside: { display: `~/.codex/skills/${bare}/SKILL.md` } }],
      });
}

/** A tool of an MCP server other than Clauseo's own, as each agent names it. */
export function mcpOf(
  agent: Agent,
  server: string,
  tool: string,
  args: object,
  status: ToolCallStatus = "completed",
) {
  return agent === "Claude"
    ? stepCall("other", `mcp__${server}__${tool}`, status, {
        tool_name: `mcp__${server}__${tool}`,
        raw: args,
      })
    : stepCall("execute", `mcp.${server}.${tool}`, status, {
        raw: { server, tool, arguments: args },
      });
}

/** A command: Claude's Bash with its description, or ChatGPT's exec with its argument list and none. */
export function commandOf(
  agent: Agent,
  command: string,
  description: string | null,
  status: ToolCallStatus = "completed",
) {
  return agent === "Claude"
    ? stepCall("execute", `\`${command}\``, status, {
        tool_name: "Bash",
        raw: description === null ? { command } : { command, description },
      })
    : stepCall("execute", command, status, {
        raw: { command: ["bash", "-lc", command], cwd: MATTER },
      });
}

/** A tool the room knows nothing of, by its own name: Claude's with its tool name, ChatGPT's as a title. */
export function unknownOf(agent: Agent, name: string, status: ToolCallStatus = "completed") {
  return agent === "Claude"
    ? stepCall("other", name, status, { tool_name: name, raw: {} })
    : stepCall("other", name, status, { raw: {} });
}

/** A helper asked with no task given: Claude's Agent untitled, ChatGPT's spawn with no prompt. */
export function helperOf(agent: Agent, status: ToolCallStatus = "completed") {
  return agent === "Claude"
    ? stepCall("think", "", status, { tool_name: "Agent", raw: {} })
    : stepCall("other", "spawnAgent", status, { raw: {} });
}

/** A page looked at online: Claude's WebFetch `{url, prompt}`, or ChatGPT's web search opening the page
 * (codex-acp's `{query, action: {type: "openPage", url}}`), with `said` as the result's text: completed with the site's
 * refusal (Claude's "The server returned HTTP 403 …"), or failed with the error. */
export function fetchOf(
  agent: Agent,
  url: string,
  status: ToolCallStatus = "completed",
  said: string | null = null,
) {
  const content = said === null ? [] : [{ text: { text: { inline: said } } }];
  return agent === "Claude"
    ? stepCall("fetch", `Fetch ${url}`, status, {
        tool_name: "WebFetch",
        raw: { url, prompt: "Find the order's date" },
        content,
        ...(said === null ? {} : { raw_output: { inline: JSON.stringify(said) } }),
      })
    : stepCall("search", `Open page: ${url}`, status, {
        raw: { query: url, action: { type: "openPage", url } },
        content,
      });
}

/** Claude's auto mode safety check's refusal, as Claude Code 0.3.293 words it. */
export const CHECKED =
  "Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Unauthorized Access] " +
  "Listing a folder outside the matter.";
/** Clauseo's own off-limits rule's refusal, as Claude Code words a deny rule's match. */
export const RULED =
  "Permission to read /Users/asha/Library/Clauseo/agents/notes.md has been denied.";

/** A step refused before it ran, `said` its refusal: by default a folder listing the safety check refused, as on
 * Windows; `kind` and `tool` for an edit or a command refused the same way. */
export function heldOf(
  status: ToolCallStatus = "failed",
  said: string = CHECKED,
  kind: ToolKind = "read",
  tool = "LS",
) {
  return stepCall(kind, "List C:\\Users\\asha\\Documents", status, {
    tool_name: tool,
    raw: { path: "C:\\Users\\asha\\Documents" },
    content: [{ text: { text: { inline: said } } }],
    raw_output: { inline: JSON.stringify(said) },
  });
}

/** One of Claude's own tools by its name (TodoWrite, AskUserQuestion …), as claude-agent-acp sends a tool it shows
 * by name: kind other, titled with the name. */
export function claudeOwn(name: string, status: ToolCallStatus = "completed") {
  return stepCall("other", name, status, { tool_name: name, raw: {} });
}

/** Claude's Yield (Claude Code 2.1.330), its wait for its own background work, as claude-agent-acp sends a tool it
 * has no reporter for: kind other, titled with the tool's name. */
export function yieldStep(status: ToolCallStatus = "completed") {
  return stepCall("other", "Yield", status, { tool_name: "Yield", raw: {} });
}

/** A long answer's steps on the Sharma matter: three papers read, a search, a skill, an Indian Kanoon search and a
 * command, as `agent` sends them. */
export function sharmaSteps(agent: Agent): ViewEntry[] {
  return [
    readOf(agent, "Plaint.docx"),
    readOf(agent, "Written statement.pdf"),
    readOf(agent, "Loan agreement, 12 March 2019.pdf"),
    searchOf(agent, "notice period"),
    skillOf(agent, "clauseo:legal-drafting"),
    mcpOf(agent, "indian-kanoon", "search_judgments", {
      query: "SARFAESI section 13(2) notice period",
    }),
    commandOf(agent, "pdfinfo Plaint.pdf | grep Pages", "Count the pages of the plaint"),
  ].map((tool_call) => ({ tool_call }));
}

/** The Windows tester's card (item C): two papers read, four pages the site refused, a page not found, a site not
 * reached, a connection refused, a page too large, and two folder listings Claude's safety check held back. */
export function testerCard(): ToolCallEntry[] {
  const ll = "https://www.livelaw.in/high-court/";
  return [
    readOf("Claude", "Plaint.docx"),
    readOf("Claude", "Written statement.pdf"),
    ...[0, 1, 2, 3].map(() =>
      fetchOf("Claude", ll, "completed", "The server returned HTTP 403 Forbidden."),
    ),
    fetchOf(
      "Claude",
      "https://delhihighcourt.nic.in/orders/123",
      "failed",
      "Request failed with status code 404",
    ),
    fetchOf(
      "Claude",
      "https://ecourts.example.in/",
      "failed",
      "getaddrinfo ENOTFOUND ecourts.example.in",
    ),
    fetchOf(
      "Claude",
      "https://www.barandbench.com/",
      "failed",
      "connect ECONNREFUSED 104.21.3.4:443",
    ),
    fetchOf(
      "Claude",
      "https://www.sci.gov.in/judgment.pdf",
      "failed",
      "maxContentLength size of 10485760 exceeded",
    ),
    heldOf(),
    heldOf(),
  ];
}

/** Claude Code's raw sentence when a code mode script fails, as an agent notice carried it before stage B's B4. */
export const SCRIPT_RAW =
  "Code that Claude ran failed with this error: Nothing ran: import and require aren't available here";

/** A notice in an answer, after the step of order `at`: by default an agent's warning with Claude's raw sentence and
 * no kind (today's shape); `kind` for a host notice (B4's "script_failed", level info, its English fallback text). */
export function noticeOf(
  at: number,
  over: { level?: NoticeEntry["level"]; kind?: string; text?: string } = {},
): ViewEntry {
  const notice: NoticeEntry = {
    order: at,
    event: uuid(900 + at, 9),
    level: over.level ?? "warning",
    text: over.text ?? SCRIPT_RAW,
    ts: 0,
    ...(over.kind === undefined ? {} : { kind: over.kind as NoticeKind }),
  };
  return { notice };
}
