// The web sources at the end of an answer (sources.ts has where they come from), icons first (DESIGN.md §Icons). Folded,
// one quiet line: up to three of its sites' tiles (the search glyph for a search that lists no pages), the count and a
// chevron. Open, one row for each search or page in the order they ran, each led by its icon: a search's glyph and its
// query in quotes, its results nested under it as tile, title and site; a page's tile, title and site, with the browser
// glyph in the tile's place for a page the browser opened and an info tip for one Codex opened on OpenAI's side. A turn
// with none draws nothing, which is every turn in the real app today. Links open through the host's open_external (its
// own confirm names the site); none navigates the webview. Tiles are letters, never favicons: nothing here makes a
// request. Queries, titles and addresses are isolated runs, marked Hindi when most of their letters are Devanagari, and
// an address wraps anywhere rather than being cut.
import { use, useState, type MouseEvent, type ReactNode } from "react";
import { siteHost } from "@clauseo/components/clauseo/site-host.ts";
import { SiteTile } from "@clauseo/components/clauseo/site-tile.tsx";
import {
  IconBrowser,
  IconFold,
  IconInfo,
  IconToolSearch,
} from "@clauseo/components/icons/icons.tsx";
import { langOfText } from "@clauseo/components/lib/script.ts";
import { useSourcesWords } from "@clauseo/components/lib/words-sources.ts";
import { InfoTip } from "@clauseo/components/ui/info-tip.tsx";
import { LayerProvider } from "@clauseo/components/ui/layer.tsx";
import { Tooltip } from "@clauseo/components/ui/tooltip.tsx";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId, StreamId, ToolCallId, WebResult } from "../../transport/protocol.ts";
import { kindOf, planName } from "../agent.ts";
import { quietly } from "../open.ts";
import {
  linkable,
  linkText,
  pagesShown,
  SourcesOpenAtFirst,
  SourcesTransport,
  useToolCallWeb,
  type ToolCallWeb,
} from "./sources.ts";

/** The most marks the folded line shows. */
const MAX_MARKS = 3;

/** Words the agent or the web wrote: isolated, marked Hindi when they are. */
function Run({ text }: { text: string }) {
  return (
    <bdi lang={langOfText(text)} className="[overflow-wrap:anywhere]">
      {text}
    </bdi>
  );
}

/** A page's title as a link, or for an untitled page its file name or else its site (linkText), never its whole
 * address. A click or a middle click never navigates the webview: it is cancelled and the address goes to the host's
 * open_external. An address that isn't http or https is plain text, with no link. */
function PageLink({ url, title }: { url: string; title: string | null }) {
  const transport = use(SourcesTransport);
  if (!linkable(url)) return <Run text={title ?? url} />;
  const open = (e: MouseEvent<HTMLAnchorElement>) => {
    e.preventDefault();
    if (transport !== null) void quietly("open_external", transport.openExternal(url));
  };
  return (
    <a
      href={url}
      rel="noreferrer noopener"
      data-testid="web-source-link"
      className="text-primary underline underline-offset-2"
      onClick={open}
      onAuxClick={open}
    >
      <Run text={linkText(url, title)} />
    </a>
  );
}

/** The site a page is on, muted after its link. Nothing for an address with no host, for one that is no link, or
 * when the link's words are the site already. */
function Site({ url, title }: { url: string; title: string | null }) {
  const site = linkable(url) ? siteHost(url) : null;
  return site === null || site === linkText(url, title) ? null : (
    <>
      {" "}
      <span className="text-muted" data-testid="web-source-site">
        <Run text={site} />
      </span>
    </>
  );
}

/** A row's leading icon, 14 px, on the first line of its words. */
function Lead({ children }: { children: ReactNode }) {
  return <span className="mt-(--s0-5) inline-flex shrink-0">{children}</span>;
}

/** The browser glyph in a page's tile's place: named, with its name as a tooltip. */
function BrowserMark() {
  const w = useSourcesWords();
  const label = w.src("Opened in the browser");
  return (
    <Tooltip content={label}>
      {/* focusable, so the keyboard reaches its tooltip as the pointer does */}
      <span
        role="img"
        aria-label={label}
        tabIndex={0}
        data-testid="web-source-browser"
        className="inline-flex rounded-ref"
      >
        <IconBrowser size={14} />
      </span>
    </Tooltip>
  );
}

/** A title from the web, null when it is empty. */
const titleOf = (title: string | undefined): string | null =>
  title === undefined || title === "" ? null : title;

function Results({ results }: { results: readonly WebResult[] }) {
  return (
    <ul className="mt-(--s0-5) grid gap-(--s0-5)">
      {results.map((r, i) => (
        <li
          key={`${i}:${r.url}`}
          data-testid="web-source-result"
          className="flex items-start gap-(--s1-5)"
        >
          <Lead>
            <SiteTile url={r.url} size={14} />
          </Lead>
          <p className="min-w-0">
            <PageLink url={r.url} title={titleOf(r.title)} />
            <Site url={r.url} title={titleOf(r.title)} />
          </p>
        </li>
      ))}
    </ul>
  );
}

type Page = ToolCallWeb["pages"][number];

/** A search's row: its query in quotes, then its results nested under it, or a muted word on what it doesn't list. */
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
  const [before, after] = w.parts("'{query}'", "query");
  return (
    <>
      <Lead>
        <IconToolSearch size={14} />
      </Lead>
      <div className="min-w-0">
        <p className="text-primary" data-testid="web-source-query">
          {before}
          <Run text={query} />
          {after}
        </p>
        {codex ? (
          <p className="mt-(--s0-5) flex items-start gap-(--s1-5)" data-testid="web-source-note">
            <Lead>
              <IconInfo size={14} />
            </Lead>
            <span>{w.src("{name} doesn't list its pages", { name })}</span>
          </p>
        ) : results.length === 0 ? (
          <p className="mt-(--s0-5)" data-testid="web-source-note">
            {w.src("No results")}
          </p>
        ) : (
          <Results results={results} />
        )}
      </div>
    </>
  );
}

/** A page's row: its tile (the browser glyph for a page the browser opened), its link and its site, and for a page
 * Codex opened, an info tip on where. */
function PageRow({ page }: { page: Page }) {
  const w = useSourcesWords();
  const title = titleOf(page.title);
  const side = w.src("Opened on OpenAI's side");
  return (
    <>
      <Lead>
        {page.via === "browser" ? <BrowserMark /> : <SiteTile url={page.url} size={14} />}
      </Lead>
      <p className="min-w-0">
        <PageLink url={page.url} title={title} />
        <Site url={page.url} title={title} />
        {page.via === "codex_open_page" && (
          <>
            {" "}
            <InfoTip label={side} className="align-[-2px]">
              {side}
            </InfoTip>
          </>
        )}
      </p>
    </>
  );
}

/** One row of the open list. */
type Item =
  | { key: string; kind: "search"; query: string; results: readonly WebResult[] }
  | { key: string; kind: "page"; page: Page };

/** The rows of a turn's tool calls' web, in the tool calls' order: each one's search, then its pages. */
/** Each answer's count: its distinct pages, the addresses its searches listed and the pages it read, an address in
 * both counted once. A search itself is no page. */
function pagesCount(items: readonly Item[]): number {
  const urls = new Set<string>();
  for (const item of items)
    if (item.kind === "page") urls.add(item.page.url);
    else for (const r of item.results) urls.add(r.url);
  return urls.size;
}

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

/** A mark of the folded line: a site's tile, or the search glyph. */
type Mark = { kind: "site"; url: string } | { kind: "search" };

/** The folded line's marks, in the rows' order: each site once (a search's by its results), the search glyph once
 * for a search that lists no pages, at most MAX_MARKS. */
function marksOf(items: readonly Item[], codex: boolean): Mark[] {
  const marks: Mark[] = [];
  const seen = new Set<string>();
  const add = (key: string, mark: Mark) => {
    if (seen.has(key) || marks.length >= MAX_MARKS) return;
    seen.add(key);
    marks.push(mark);
  };
  const site = (url: string) => add(`site:${siteHost(url) ?? url}`, { kind: "site", url });
  for (const item of items) {
    if (item.kind === "page") site(item.page.url);
    else if (codex || item.results.length === 0) add("search", { kind: "search" });
    else for (const r of item.results) site(r.url);
  }
  return marks;
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
  const codex = agent !== undefined && kindOf(agent, []) === "codex";
  const pages = pagesCount(items);
  // the thread's plan names it
  const name = agent === undefined ? planName("codex-acp") : planName(agent);
  return (
    // the info tip and the browser glyph's tooltip mount here, inside the thread, which stands in front of the end of
    // the body (.room-centre's z-index): mounted there they would open behind it, unseen
    <LayerProvider>
      <div data-testid="web-sources" className="not-first:mt-(--s3)">
        <button
          type="button"
          className="work-done"
          data-testid="web-sources-toggle"
          aria-expanded={open}
          // searches that list no page: no count to say, so the button is named for what it opens to
          aria-label={pages === 0 ? w.src("Searches") : undefined}
          data-panel-open={open ? "" : undefined}
          onClick={() => setOpen((o) => !o)}
        >
          {/* the sites' tiles lead; the words count the distinct pages, none when there are none */}
          <span className="inline-flex items-center gap-(--s0-5)" data-testid="web-sources-marks">
            {marksOf(items, codex).map((m) =>
              m.kind === "search" ? (
                <IconToolSearch key="search" size={11} />
              ) : (
                <SiteTile key={m.url} url={m.url} size={11} />
              ),
            )}
          </span>
          {pages > 0 && <span data-testid="web-sources-count">{w.summary(pages)}</span>}
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
                {s.kind === "search" ? (
                  <SearchRow query={s.query} results={s.results} codex={codex} name={name} />
                ) : (
                  <PageRow page={s.page} />
                )}
              </li>
            ))}
          </ol>
        )}
      </div>
    </LayerProvider>
  );
}
