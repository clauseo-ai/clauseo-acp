// The web sources at the end of an answer (sources.ts has where they come from): one quiet line, shut at first, that
// sums them up like the step group's chip ("2 web searches. 3 pages read.", the kinds' icons, a chevron), and opened,
// one row for each search or page in the order they ran. A turn with none draws nothing, which is every turn in the
// real app today. Links open through the host's open_external (its own confirm names the site); none navigates the
// webview. Queries, titles and addresses are isolated runs, marked Hindi when most of their letters are Devanagari,
// and an address wraps anywhere rather than being cut.
import { use, useState, type ReactNode } from "react";
import { IconFold, IconToolFetch, IconToolSearch } from "@clauseo/components/icons/icons.tsx";
import { useSourcesWords, type SourcesKey } from "@clauseo/components/lib/words-sources.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId, StreamId, ToolCallId, WebResult } from "../../transport/protocol.ts";
import { kindOf, planName } from "../agent.ts";
import { quietly } from "../open.ts";
import {
  pagesShown,
  SourcesOpenAtFirst,
  SourcesTransport,
  useToolCallWeb,
  type ToolCallWeb,
} from "./sources.ts";

const DEVANAGARI = /\p{Script=Devanagari}/gu;
const LETTER = /\p{L}/gu;

/** "hi" when most of the text's letters are Devanagari. Private until #287's langOfText lands, which it gives way to. */
function langOfText(text: string): "hi" | undefined {
  const letters = text.match(LETTER)?.length ?? 0;
  const hindi = text.match(DEVANAGARI)?.length ?? 0;
  return letters > 0 && hindi * 2 > letters ? "hi" : undefined;
}

/** The site an address is on, without its "www."; null for an address that doesn't parse. */
function siteOf(url: string): string | null {
  try {
    const host = new URL(url).host;
    return host === "" ? null : host.replace(/^www\./, "");
  } catch {
    return null;
  }
}

/** Words the agent or the web wrote: isolated, marked Hindi when they are. */
function Run({ text }: { text: string }) {
  return (
    <bdi lang={langOfText(text)} className="[overflow-wrap:anywhere]">
      {text}
    </bdi>
  );
}

/** A page's title as a link, or its address when it has none, then its site, muted. */
function PageLink({ url, title }: { url: string; title: string | null }) {
  const transport = use(SourcesTransport);
  const site = siteOf(url);
  return (
    <>
      <a
        href={url}
        data-testid="web-source-link"
        className="text-primary underline underline-offset-2"
        onClick={(e) => {
          e.preventDefault();
          if (transport !== null) void quietly("open_external", transport.openExternal(url));
        }}
      >
        <Run text={title ?? url} />
      </a>
      {site !== null && (
        <>
          {" "}
          <span className="text-muted">
            <Run text={site} />
          </span>
        </>
      )}
    </>
  );
}

/** A string with one slot filled by an element. */
function Slotted({
  k,
  slot,
  name,
  children,
}: {
  k: SourcesKey;
  slot: string;
  name?: string;
  children: ReactNode;
}) {
  const w = useSourcesWords();
  const [before, after] = w.parts(k, slot, name === undefined ? undefined : { name });
  return (
    <>
      {before}
      {children}
      {after}
    </>
  );
}

function Results({ results }: { results: readonly WebResult[] }) {
  return (
    <ul className="mt-(--s0-5) grid gap-(--s0-5)">
      {results.map((r, i) => (
        <li key={`${i}:${r.url}`} data-testid="web-source-result">
          <PageLink url={r.url} title={r.title === "" ? null : r.title} />
        </li>
      ))}
    </ul>
  );
}

type Page = ToolCallWeb["pages"][number];

/** A search's row: its query, then each result on a line of its own, or what the agent doesn't tell. */
function SearchRow({
  query,
  results,
  codex,
  name,
}: {
  query: string;
  results: readonly WebResult[];
  codex: boolean;
  name: string;
}) {
  const w = useSourcesWords();
  return (
    <>
      <p>
        <Slotted k="Searched the web for {query}." slot="query">
          <Run text={query} />
        </Slotted>
        {codex
          ? ` ${w.src("{name} doesn't report which pages it found.", { name })}`
          : results.length === 0
            ? ` ${w.src("No results.")}`
            : null}
      </p>
      {!codex && results.length > 0 && <Results results={results} />}
    </>
  );
}

/** A page's row, by how it was opened. */
function PageRow({ page, name }: { page: Page; name: string }) {
  const w = useSourcesWords();
  const link = <PageLink url={page.url} title={page.title ?? null} />;
  if (page.via === "browser")
    return (
      <p>
        <Slotted k="Opened {page} in the browser." slot="page">
          {link}
        </Slotted>
      </p>
    );
  const read = (
    <p>
      <Slotted k="Read {page}." slot="page">
        {link}
      </Slotted>
    </p>
  );
  if (page.via !== "codex_open_page") return read;
  return (
    <>
      {read}
      <p className="text-muted">
        {w.src("{name} opened this page on OpenAI's computers, not on this one.", { name })}
      </p>
    </>
  );
}

/** One row of the open list. */
type Item =
  | { key: string; kind: "search"; query: string; results: readonly WebResult[] }
  | { key: string; kind: "page"; page: Page };

/** The rows of a turn's tool calls' web, in the tool calls' order: each one's search, then its pages. */
function itemsOf(webs: readonly (readonly [ToolCallId, ToolCallWeb])[]): Item[] {
  return webs.flatMap(([call, web]) => [
    ...(web.search === undefined
      ? []
      : [
          {
            key: `${call}:search`,
            kind: "search" as const,
            query: web.search.query,
            results: web.search.results,
          },
        ]),
    ...pagesShown(web.pages).map((page, i) => ({
      key: `${call}:${i}`,
      kind: "page" as const,
      page,
    })),
  ]);
}

/** The web sources at the end of one turn's answer: its tool calls' web, by id, in order; nothing when none found
 * anything. `agent`: the thread's, which ran every search and page (ToolCallWeb names none). */
export function WebSources({
  stream,
  calls,
  agent,
}: {
  stream: StreamId;
  calls: readonly ToolCallId[];
  agent: AgentId | undefined;
}) {
  const transport = use(SourcesTransport);
  if (transport === null || calls.length === 0) return null;
  return <SourcesOf transport={transport} stream={stream} calls={calls} agent={agent} />;
}

function SourcesOf({
  transport,
  stream,
  calls,
  agent,
}: {
  transport: HostTransport;
  stream: StreamId;
  calls: readonly ToolCallId[];
  agent: AgentId | undefined;
}) {
  const w = useSourcesWords();
  const webs = useToolCallWeb(transport, stream);
  // each answer's own, while it is mounted; shut at first
  const [open, setOpen] = useState(use(SourcesOpenAtFirst));
  const items = itemsOf(
    calls.flatMap((call) => {
      const web = webs.get(call);
      return web === undefined ? [] : [[call, web] as const];
    }),
  );
  if (items.length === 0) return null;
  const searches = items.filter((s) => s.kind === "search").length;
  const pages = items.length - searches;
  const codex = agent !== undefined && kindOf(agent, []) === "codex";
  // the thread's plan names it; a page opened on OpenAI's computers is always ChatGPT's
  const name = agent === undefined ? planName("codex-acp") : planName(agent);
  return (
    <div data-testid="web-sources" className="not-first:mt-(--s3)">
      <button
        type="button"
        className="work-done"
        data-testid="web-sources-toggle"
        aria-expanded={open}
        data-panel-open={open ? "" : undefined}
        onClick={() => setOpen((o) => !o)}
      >
        {searches > 0 && <IconToolSearch size={11} />}
        {pages > 0 && <IconToolFetch size={11} />}
        <span>{w.summary(searches, pages)}</span>
        <IconFold size={12} className="work-chev" />
      </button>
      {open && (
        <ol className="work-list">
          {items.map((s) => (
            <li
              key={s.key}
              data-testid="web-source"
              data-kind={s.kind === "search" ? "search" : s.page.via}
              className="flex items-start gap-(--s2)"
            >
              <span className="mt-(--s0-5) shrink-0">
                {s.kind === "search" ? <IconToolSearch size={14} /> : <IconToolFetch size={14} />}
              </span>
              <div className="min-w-0">
                {s.kind === "search" ? (
                  <SearchRow query={s.query} results={s.results} codex={codex} name={name} />
                ) : (
                  <PageRow
                    page={s.page}
                    name={s.page.via === "codex_open_page" && !codex ? planName("codex-acp") : name}
                  />
                )}
              </div>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
