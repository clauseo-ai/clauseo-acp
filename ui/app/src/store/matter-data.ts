// The matter data the room's parts read (ui/components MatterData), assembled from what the host says about a matter,
// with one check: every citation names a paper the matter holds. A citation that names any other paper is logged and
// dropped, so no part ever shows a reference to a paper the lawyer does not have.
//
// The protocol carries no papers or citations yet, so today the window assembles it from empty lists; the check runs
// on whatever arrives once it does.
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
