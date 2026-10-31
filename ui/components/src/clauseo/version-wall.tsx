import { Button } from "../ui/button.tsx";
import { useWords } from "../lib/words.ts";
import { vt } from "../lib/words-versions.ts";
import { Heading } from "./heading.tsx";
import {
  citationsOf,
  citedPapers,
  pbWords,
  shortTitle,
  type Cite,
  type Citation,
  type Paper,
  type TileState,
} from "./matter.ts";
import { useMatter } from "./matter-context.ts";
import { PaperTile } from "./paper-tile.tsx";
import { useRoom } from "./room-scope.ts";
import { eraOf, type Version } from "./versions-model.ts";
import { Citations } from "./citations.tsx";

/** One of Clauseo's own papers at the wall's end, such as an accepted list of dates or a prepared draft. */
export type OwnPaper = {
  id: string;
  /** Its tile's title, as filed. */
  title: string;
  state: "accepted" | "prepared";
  /** The word standing where a page number would: "Dates", "Draft". */
  word: string;
};

/** The wall as it stood at a version: the matter's papers in paper book order, under "Papers" and their count then.
 * Papers not in the matter yet leave the wall; the papers the version changed come forward. At now, the wall is the
 * room's own. Clauseo's own papers end it. The papers and judgments come from the props, or from useMatter(). */
export function VersionWall({
  version,
  looking,
  own,
  copy = null,
  papers,
  citations,
  cites,
}: {
  version: Version;
  looking: boolean;
  /** Clauseo's own papers, in the order they end the wall. */
  own: readonly OwnPaper[];
  /** A new draft saved from an earlier version: its tile's title, set beside the prepared draft. Flat, a paper of the
   * matter's like any other: nothing waits on it. */
  copy?: string | null;
  /** The matter's papers in paper book order; the matter in reach when left out. */
  papers?: readonly Paper[];
  /** The answer's citations of the papers; the answer in view's when left out. */
  citations?: readonly Citation[];
  /** The judgments the draft cites, by id; the matter in reach when left out. */
  cites?: Readonly<Record<string, Cite>>;
}) {
  const w = useWords();
  const { id } = useRoom();
  const matter = useMatter();
  const all = papers ?? matter.papers;
  const lit = citedPapers(citations ?? citationsOf(matter, matter.inView), all);
  const cited = cites ?? matter.cites;
  const count = all.filter((p) => version.state.papers.includes(p.id)).length;
  const era = (p: string) => {
    if (!looking) return undefined;
    const e = eraOf(p, version);
    return e === "gone" ? "gone" : e === "touched" ? "touched" : undefined;
  };
  const sr = (p: string) => {
    const e = era(p);
    return e === "touched"
      ? vt(w.lang, "Changed in this version")
      : e === undefined
        ? null
        : vt(w.lang, "Not in the matter yet");
  };
  return (
    <>
      <section className="flex flex-col" aria-labelledby={id("papers-head")}>
        <header className="wall-head">
          <Heading level={2} className="wall-title" id={id("papers-head")}>
            {w.t("Papers")} <span className="wall-count ver-count">{count}</span>
          </Heading>
          <p className="wall-sub">{w.t("By paper book page")}</p>
          <Button kind="quiet" size="small" className="wall-action">
            {w.t("All {n}", { n: count })}
          </Button>
        </header>
        <ol className="facade ver-facade" data-looking={looking ? "" : undefined}>
          {all.map((paper) => {
            const more = sr(paper.id);
            const role = lit.get(paper.id)?.citation.role;
            const state: TileState = role ?? "uncited";
            const label = w.t("{title}, {pb}", {
              title: paper.title,
              pb: pbWords(w.lang, paper.pb),
            });
            return (
              <PaperTile
                key={paper.id}
                id={paper.id}
                state={state}
                numeral={paper.pb}
                title={shortTitle(paper.title)}
                label={more === null ? label : `${label}. ${more}`}
                card={role !== undefined && !looking}
                era={era(paper.id)}
              />
            );
          })}
          {own.flatMap((o) => {
            const tile = (
              <PaperTile
                key={o.id}
                id={o.id}
                state={o.state}
                numeral={{ word: o.word }}
                title={o.title}
                label={sr(o.id) ?? o.title}
                era={era(o.id)}
              />
            );
            if (copy === null || looking || o.state !== "prepared") return [tile];
            const copyId = `${o.id}-copy`;
            return [
              tile,
              <PaperTile
                key={copyId}
                id={copyId}
                state="uncited"
                numeral={{ word: o.word }}
                title={copy}
                label={copy}
              />,
            ];
          })}
        </ol>
      </section>
      <Citations
        layout="draft"
        cites={Object.entries(cited).map(([c, cite]) => ({ id: c, cite, checked: true }))}
      />
    </>
  );
}
