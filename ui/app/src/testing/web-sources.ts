// Tool call entries that carry `web` (ToolCallWeb, 0041), as the host's reducer puts it on them, for tests and the copy
// review (room/thread/sources.tsx): a search and the pages a tool call opened, each entry titled as the agent's adapter
// titles it.
import type {
  AgentId,
  PageVia,
  ToolCallId,
  ToolCallPage,
  ToolCallSearch,
  ToolCallWeb,
  ViewEntry,
  WebResult,
} from "../transport/protocol.ts";
import { header, TURN } from "./stream-script.ts";

/** A search a tool call ran, with the results the agent got, in their order. */
export const searchOf = (query: string, results: WebResult[] = []): ToolCallSearch => ({
  query,
  results,
});

/** A page a tool call opened, `via` web_fetch by default, with its title when it has one. */
export const pageOf = (
  url: string,
  { title, via = "web_fetch" }: { title?: string; via?: PageVia } = {},
): ToolCallPage => ({ url, ...(title === undefined ? {} : { title }), via });

/** A tool call's web: its search, when it ran one, and the pages it opened, in order. */
export const webOf = ({
  search,
  pages = [],
}: { search?: ToolCallSearch; pages?: ToolCallPage[] } = {}): ToolCallWeb => ({
  ...(search === undefined ? {} : { search }),
  pages,
});

/** The title the agent's adapter gives a web tool call: claude-agent-acp titles a search `Search "<query>"` and a fetch
 * `Fetch <url>`; codex-acp titles its search `Searching for: <query>` and its open page `Opening: <url>`. The browser's
 * page is titled by its address alone (#275 records no title for it). A call with a search is titled by the search. */
export function titleOf(web: ToolCallWeb, agent: AgentId): string {
  const codex = agent.startsWith("codex");
  if (web.search !== undefined)
    return codex ? `Searching for: ${web.search.query}` : `Search "${web.search.query}"`;
  const page = web.pages[0];
  if (page === undefined) return "";
  if (page.via === "codex_open_page") return `Opening: ${page.url}`;
  if (page.via === "browser") return page.url;
  return `Fetch ${page.url}`;
}

/** A completed tool call entry at `order` that carries `web` (the reducer's view of it), titled as titleOf gives it. */
export const webCall = (
  order: number,
  call: ToolCallId,
  web: ToolCallWeb,
  agent: AgentId = "claude-acp",
): ViewEntry => ({
  tool_call: {
    order,
    tool_call: call,
    turn: TURN,
    started_at: order * 1000,
    title: titleOf(web, agent),
    kind: "fetch",
    status: "completed",
    locations: [],
    content: [],
    web,
  },
});

/** The thread's header, its agent `agent`. */
export const headerOf = (agent: AgentId): ViewEntry => {
  const head = header();
  head.header!.agent = agent;
  return head;
};
