// The matter data the room's parts read (ui/components MatterData), assembled from what the host says about a matter,
// with one check: every citation names a paper the matter holds. A citation that names any other paper is logged and
// dropped here; the parts drop such a citation too (they never show a reference to a paper the lawyer does not have),
// so this layer's drop changes nothing they show, and its log is the record of it.
//
// Nothing builds MatterData yet: the protocol carries no papers or citations, so the store has nothing to build it
// from. The check is ready for when they arrive; the store calls it then, where it builds the matter's view.
import type {
  AnswerId,
  Citation,
  Cite,
  CiteId,
  MatterData,
  Paper,
} from "@clauseo/components/clauseo/matter.ts";

export type MatterSource = {
  readonly papers: readonly Paper[];
  readonly cites?: Readonly<Record<CiteId, Cite>>;
  readonly citations?: Readonly<Record<AnswerId, readonly Citation[]>>;
  readonly inView?: AnswerId | null;
};

/** MatterData from a matter's papers and its answers' citations; `warn` hears of each citation whose paper the matter
 * does not hold. */
export function buildMatterData(
  source: MatterSource,
  warn: (message: string) => void = (m) => console.warn(m),
): MatterData {
  const held = new Set(source.papers.map((p) => p.id));
  const citations: Record<AnswerId, readonly Citation[]> = {};
  for (const [answer, list] of Object.entries(source.citations ?? {})) {
    const kept = list.filter((c) => {
      if (held.has(c.paperId)) return true;
      warn(
        `answer ${answer}: citation ${c.id} names paper ${c.paperId}, which the matter does not hold`,
      );
      return false;
    });
    citations[answer] = kept;
  }
  return {
    papers: source.papers,
    cites: source.cites ?? {},
    citations,
    inView: source.inView ?? null,
  };
}
