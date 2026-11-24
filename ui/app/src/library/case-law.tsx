// Clauseo Case Law, at the head of Connectors: its card and the blocks its detail adds. Not added,
// the card offers it in the agreed words (shared with the home card, UC-1090 to UC-1095) and its
// one button opens today's add flow, Settings at Connectors. Added, it shows the credits left. When
// stage D can't reach it (`unreachable`), it keeps its name and line, says so with a yellow dot,
// and offers Retry. The price and credits are the connectors' state's.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { ConnectorView, MatterId } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import { creditLines, creditsOf, priceLine } from "./case-law-model.ts";
import { ItemTile } from "./glyphs.tsx";
import { descriptionOf, nameOf, scopeTag, troubleTag } from "./model.ts";

export type CaseLawActs = {
  /** The connector as the connectors' state lists it, for its price and credits. */
  view: ConnectorView | undefined;
  /** Today's add flow (Settings at Connectors); null where there is no settings window. */
  onAdd: (() => void) | null;
  /** Its reach checked again; null where it can't be. */
  onRetry: (() => void) | null;
};

type Names = ReadonlyMap<MatterId, string>;

function Meta({ a, acts, names }: { a: AddonView; acts: CaseLawActs; names: Names }) {
  const { lang } = useDisplay();
  if (a.status === "needs_setup") {
    const price = priceLine(lang, acts.view);
    return (
      <span className="lib-feature-meta">
        {price !== null && <span className="lib-credits">{price}</span>}
        <span>{cr(lang, "Prepaid, including GST. No subscription.")}</span>
      </span>
    );
  }
  const trouble =
    a.status === "unreachable" ? lb(lang, "Can't reach it right now") : troubleTag(lang, a.status);
  if (trouble !== null)
    return (
      <span className="lib-feature-meta">
        <span className="lib-row-tag" data-trouble="" data-testid="case-law-trouble">
          {trouble}
        </span>
      </span>
    );
  const credits = creditsOf(acts.view);
  return (
    <span className="lib-feature-meta">
      {credits !== null &&
        creditLines(lang, credits).map((line, i) => (
          <span key={line} className={i === 0 ? "lib-credits" : undefined}>
            {line}
          </span>
        ))}
      <span>{scopeTag(lang, a, names)}</span>
    </span>
  );
}

export function CaseLawCard({
  a,
  acts,
  names,
  selected,
  onSelect,
}: {
  a: AddonView;
  acts: CaseLawActs;
  names: Names;
  selected: boolean;
  onSelect: () => void;
}) {
  const { lang } = useDisplay();
  const offer = a.status === "needs_setup";
  const action =
    offer && acts.onAdd !== null ? (
      <Button kind="primary" className="lib-feature-add" onClick={acts.onAdd}>
        {cr(lang, "Add Clauseo Case Law")}
      </Button>
    ) : a.status === "unreachable" && acts.onRetry !== null ? (
      <Button kind="tone" className="lib-feature-add" onClick={acts.onRetry}>
        {lb(lang, "Retry")}
      </Button>
    ) : null;
  return (
    <div
      className="lib-feature"
      data-act={action === null ? undefined : ""}
      data-testid="case-law-card"
      data-status={a.status}
      aria-current={selected ? "true" : undefined}
    >
      <button type="button" className="lib-feature-hit" onClick={onSelect}>
        <ItemTile addon={a} big />
        <span className="lib-feature-text">
          <span className="lib-feature-name">{nameOf(lang, a)}</span>
          <span className="lib-powered">{cr(lang, "Powered by Indian Kanoon")}</span>
          <span className="lib-feature-line">{descriptionOf(lang, a)}</span>
          <Meta a={a} acts={acts} names={names} />
        </span>
      </button>
      {action}
    </div>
  );
}

/** What the detail adds for Clauseo Case Law: before it is added, what it covers and costs, and Add
 * again; added, its credits and Add credits (today's flow, in Settings). */
export function CaseLawBlocks({ a, acts, who }: { a: AddonView; acts: CaseLawActs; who: string }) {
  const { lang } = useDisplay();
  const cost = lb(
    lang,
    "A page of search results is 20 credits. Opening a judgment is 3 credits, plus 1 for each 4,000 tokens {who} reads (about 3,000 words).",
    { who },
  );
  if (a.status === "needs_setup") {
    const price = priceLine(lang, acts.view);
    return (
      <section className="lib-detail-block">
        <h4 className="lib-detail-label">{lb(lang, "What it covers")}</h4>
        <p className="text-ui text-primary">
          {lb(
            lang,
            "Judgments of the Supreme Court, the High Courts and tribunals, and central Acts. For any judgment, {who} can see which later judgments cite it, and how.",
            { who },
          )}
        </p>
        <h4 className="lib-detail-label mt-s3">{lb(lang, "What it costs")}</h4>
        <p className="text-ui text-primary">
          {price === null ? "" : `${price}. `}
          {cr(lang, "Prepaid, including GST. No subscription.")}
        </p>
        <p className="text-small text-muted">{cost}</p>
        {acts.onAdd !== null && (
          <div className="lib-detail-acts">
            <Button kind="primary" onClick={acts.onAdd}>
              {cr(lang, "Add Clauseo Case Law")}
            </Button>
          </div>
        )}
      </section>
    );
  }
  if (a.status !== "ready") return null;
  const credits = creditsOf(acts.view);
  return (
    <section className="lib-detail-block">
      {credits !== null && (
        <p className="text-ui text-primary">
          <span className="lib-credits">{creditLines(lang, credits).join(" · ")}</span>
        </p>
      )}
      <p className="text-small text-muted">{cost}</p>
      {acts.onAdd !== null && (
        <div className="lib-detail-acts">
          <Button kind="tone" size="small" onClick={acts.onAdd}>
            {cr(lang, "Add credits")}
          </Button>
        </div>
      )}
    </section>
  );
}
