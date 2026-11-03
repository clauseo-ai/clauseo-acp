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
import type { AgentId, StreamId, TurnId, WebResult } from "../../transport/protocol.ts";
import { kindOf, planName } from "../agent.ts";
import { quietly } from "../open.ts";
import { SourcesTransport, useWebSources, type WebSource } from "./sources.ts";

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

function SearchRow({ source }: { source: Extract<WebSource, { kind: "search" }> }) {
  const w = useSourcesWords();
  const codex = kindOf(source.agent, []) === "codex";
  const name = planName(source.agent);
  const query = (
    <Slotted k="Searched the web for {query}." slot="query">
      <Run text={source.query} />
    </Slotted>
  );
  return (
    <>
      <p>
        {query}
        {codex
          ? ` ${w.src("{name} doesn't report which pages it found.", { name })}`
          : source.results.length === 0
            ? ` ${w.src("No results.")}`
            : null}
      </p>
      {!codex && source.results.length > 0 && <Results results={source.results} />}
    </>
  );
}

function PageRow({
  source,
  agent,
}: {
  source: Extract<WebSource, { kind: "page" }>;
  agent: AgentId | undefined;
}) {
  const w = useSourcesWords();
  const link = <PageLink url={source.url} title={source.title} />;
  if (source.via === "browser")
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
  if (source.via !== "codex_open_page") return read;
  // only Codex opens a page this way: the thread's own plan names it, ChatGPT's when the thread's agent isn't known
  const name =
    agent !== undefined && kindOf(agent, []) === "codex" ? planName(agent) : planName("codex-acp");
  return (
    <>
      {read}
      <p className="text-muted">
        {w.src("{name} opened this page on OpenAI's computers, not on this one.", { name })}
      </p>
    </>
  );
}

/** The sources of one turn's answer, at its end; nothing for a turn with none. `agent`: the thread's. */
export function WebSources({
  stream,
  turn,
  agent,
}: {
  stream: StreamId;
  turn: TurnId | undefined;
  agent: AgentId | undefined;
}) {
  const transport = use(SourcesTransport);
  if (transport === null || turn === undefined) return null;
  return <SourcesOf transport={transport} stream={stream} turn={turn} agent={agent} />;
}

function SourcesOf({
  transport,
  stream,
  turn,
  agent,
}: {
  transport: HostTransport;
  stream: StreamId;
  turn: TurnId;
  agent: AgentId | undefined;
}) {
  const w = useSourcesWords();
  const sources = useWebSources(transport, stream, turn);
  // each answer's own, while it is mounted; shut at first
  const [open, setOpen] = useState(false);
  if (sources.length === 0) return null;
  const searches = sources.filter((s) => s.kind === "search").length;
  const pages = sources.length - searches;
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
          {sources.map((s) => (
            <li
              key={s.seq}
              data-testid="web-source"
              data-kind={s.kind === "search" ? "search" : s.via}
              className="flex items-start gap-(--s2)"
            >
              <span className="mt-(--s0-5) shrink-0">
                {s.kind === "search" ? <IconToolSearch size={14} /> : <IconToolFetch size={14} />}
              </span>
              <div className="min-w-0">
                {s.kind === "search" ? (
                  <SearchRow source={s} />
                ) : (
                  <PageRow source={s} agent={agent} />
                )}
              </div>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
