// Find's results on the rail (U16a), in their own chunk (find-load.ts loads it as the field is hovered or focused): the
// search inside (store/search.ts) for the words in the field, shaped by find.ts and drawn as the approved page draws
// them. A matter opens as from its row; a conversation opens, and one found in something said opens at that entry
// (thread/jump-entry.ts); a paper opens its matter and the paper in its own app, as a paper named in an answer does
// (open.ts): there is no reader in the room yet. More hits come a page at a time as the rail is scrolled to its foot.
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ComponentType,
} from "react";
import { Heading } from "@clauseo/components/clauseo/heading.tsx";
import {
  MatterRow,
  type FindSearch,
  type MatterRowProps,
} from "@clauseo/components/clauseo/rail.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { fileTypeIcon } from "@clauseo/components/icons/file-type.ts";
import type { IconProps } from "@clauseo/components/icons/icon.tsx";
import { IconAbout, IconArchived, IconChecking } from "@clauseo/components/icons/icons.tsx";
import { isolate } from "@clauseo/components/lib/bidi.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { langOfText } from "@clauseo/components/lib/script.ts";
import { fw } from "@clauseo/components/lib/words-find.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { Tooltip } from "@clauseo/components/ui/tooltip.tsx";
import { Search } from "../store/search.ts";
import { streamOf } from "../store/view.ts";
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { MatterId, SnippetPart } from "../transport/protocol.ts";
import type { Conversations, ConversationsView } from "./conversations.ts";
import {
  archivedIn,
  archivedSummary,
  findView,
  type FoundConversation,
  type FoundPaper,
} from "./find.ts";
import type { Matters, MattersView } from "./matters.ts";
import { openConversation } from "./open-conversation.ts";
import type { AllThreadsView } from "./all-threads.ts";
import { quietly } from "./open.ts";
import { clearJump, jumpTo } from "./thread/jump-entry.ts";

export type FindResultsProps = {
  transport: HostTransport;
  search: FindSearch;
  matters: Matters;
  conversations: Conversations;
  view: MattersView;
  talks: ConversationsView;
  /** Each matter's row, as the rail draws it. */
  rows: ReadonlyMap<MatterId, MatterRowProps>;
  /** Every matter's threads (all-threads.ts), archived ones included: a hit's archived mark comes from here. */
  every: AllThreadsView;
  /** A matter or a paper was opened. */
  onMoved: () => void;
  /** A conversation was opened. */
  onConversation: () => void;
};

/** How near the rail's foot, in px, the next page is asked for. */
const FOOT = 160;

/** Words as filed or as said, in the language their script says: a Devanagari run is Hindi, anything else English. */
function Own({ text }: { text: string }) {
  return langOfText(text) === "hi" ? (
    <span lang="hi">{text}</span>
  ) : (
    <span className="as-en" lang="en">
      {text}
    </span>
  );
}

/** A snippet as text, its found words marked in primary at chip weight: each part a span, never markup. Isolated (a
 * <bdi>), as every path is (docs/ui-states.md): a file name's right-to-left marks can't reorder the line around it. */
function Parts({ parts }: { parts: readonly SnippetPart[] }) {
  return (
    <bdi lang={langOfText(parts.map((p) => p.text).join("")) === "hi" ? "hi" : "en"}>
      {parts.map((p, i) =>
        p.hit ? (
          <span key={i} className="room-find-hit">
            {p.text}
          </span>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </bdi>
  );
}

function PaperRow({
  paper,
  Glyph,
  onOpen,
}: {
  paper: FoundPaper;
  /** The paper's file type's glyph (file-type.ts). */
  Glyph: ComponentType<IconProps>;
  onOpen: () => void;
}) {
  const { lang } = useDisplay();
  return (
    <li>
      <button type="button" className="room-matter-row" data-testid="find-paper" onClick={onOpen}>
        <span className="room-matter-name">
          <Own text={paper.name} />
        </span>
        <span className="room-matter-match room-find-paper">
          <Glyph size={14} />
          <span className="sr-only">
            {fw(lang, "Paper, {file}", { file: isolate(paper.file) })}
          </span>
          <span aria-hidden="true">
            <Parts parts={paper.parts} />
          </span>
        </span>
      </button>
    </li>
  );
}

function ConversationRow({ found, onOpen }: { found: FoundConversation; onOpen: () => void }) {
  const { lang } = useDisplay();
  return (
    <li>
      <button
        type="button"
        className="room-matter-row"
        data-testid="find-conversation"
        data-order={found.order}
        onClick={onOpen}
      >
        <span className="room-matter-name">
          <Own text={found.title} />
          {found.archived && (
            <span className="room-find-archived" title={fw(lang, "Archived")}>
              <IconArchived size={14} />
              <span className="sr-only">{fw(lang, "Archived")}</span>
            </span>
          )}
        </span>
        <span className="room-matter-note">
          <Say>{found.where}</Say>
        </span>
        {found.parts !== undefined && (
          <span className="room-matter-match room-find-said">
            <span className="text-quiet">{found.who}</span> <Parts parts={found.parts} />
          </span>
        )}
      </button>
    </li>
  );
}

export function FindResults({
  transport,
  search: field,
  matters,
  conversations,
  view,
  talks,
  rows,
  every,
  onMoved,
  onConversation,
}: FindResultsProps) {
  const { lang } = useDisplay();
  const [search] = useState(() => new Search(transport));
  useEffect(() => search.start(), [search]);
  // every change of the words, a search: the host supersedes the older ones, so nothing waits here
  useEffect(() => search.set(field.query), [search, field.query]);
  const state = useSyncExternalStore(search.subscribe, search.state, search.state);
  const archived = archivedIn(every, talks);
  const shown = findView({
    lang,
    query: field.query,
    matching: field.matching,
    search: state,
    matters: view.matters,
    rows,
    archived,
    now: view.now,
    zone: view.zone,
  });
  const foot = useRef<HTMLParagraphElement>(null);
  // the next page as the rail nears its foot: read on scroll and as hits come in, so nothing runs at rest
  useLayoutEffect(() => {
    const rail = foot.current?.closest<HTMLElement>(".room-rail");
    if (rail === null || rail === undefined || state.next === null) return;
    const look = () => {
      if (rail.clientHeight === 0) return;
      if (rail.scrollTop + rail.clientHeight >= rail.scrollHeight - FOOT) search.more();
    };
    look();
    rail.addEventListener("scroll", look, { passive: true });
    return () => rail.removeEventListener("scroll", look);
  }, [search, state.next, state.hits]);
  const openTalk = (found: FoundConversation) => () => {
    // asked before it opens, so the thread's first rows already page back to the entry
    if (found.order !== undefined) jumpTo(streamOf(found.thread), found.order);
    else clearJump();
    const known = found.archived
      ? archivedSummary(every, found.matter, found.thread, talks.agent)
      : undefined;
    openConversation({ matters, conversations }, found.matter, found.thread, known);
    field.leave();
    onConversation();
  };
  const openPaper = (paper: FoundPaper) => () => {
    if (view.open?.matter !== paper.matter) matters.open(paper.matter);
    field.leave();
    onMoved();
    const papers = matters.next.papers;
    if (papers === null) return;
    papers.listPapers(paper.matter).then(
      ({ papers: listed }) => {
        const found = listed.find((p) => p.path === paper.path);
        if (found !== undefined) void quietly("open_file", transport.openFile(found.file));
      },
      (error: unknown) => console.warn(`list_papers failed: ${toHostError(error).code}`),
    );
  };
  const scope = fw(lang, "Searches titles, paper names and conversations.");
  return (
    <>
      {shown.indexing && (
        <p className="room-find-indexing" role="status">
          <IconChecking size={14} className="room-matter-mark" />
          <span className="deva-nudge">
            <Say>{fw(lang, "Still adding older conversations.")}</Say>
          </span>
        </p>
      )}
      {shown.matters.length + shown.papers.length > 0 && (
        <section>
          <Heading level={2} depth={1} className="room-when">
            {mt(lang, "Matching matters")}
          </Heading>
          <ul className="room-matters">
            {shown.matters.map((m) => (
              <MatterRow
                key={m.id ?? m.name}
                {...m}
                onOpen={() => {
                  field.leave();
                  m.onOpen?.();
                }}
              />
            ))}
            {shown.papers.map((p) => (
              <PaperRow key={p.key} paper={p} Glyph={fileTypeIcon(p.file)} onOpen={openPaper(p)} />
            ))}
          </ul>
        </section>
      )}
      {shown.conversations.length > 0 && (
        <section>
          <Heading level={2} depth={1} className="room-when">
            {fw(lang, "Conversations")}
          </Heading>
          <ul className="room-matters">
            {shown.conversations.map((c) => (
              <ConversationRow key={c.key} found={c} onOpen={openTalk(c)} />
            ))}
          </ul>
        </section>
      )}
      {shown.nothing && (
        <div className="room-find-none" role="status">
          <p className="text-ui font-chip text-primary">
            <Say>{fw(lang, 'Nothing found for "{words}".', { words: field.query.trim() })}</Say>
          </p>
        </div>
      )}
      <p ref={foot} className="room-find-foot">
        <Tooltip content={scope}>
          <button type="button" className="room-find-scope" aria-label={scope}>
            <IconAbout size={14} />
          </button>
        </Tooltip>
      </p>
    </>
  );
}
