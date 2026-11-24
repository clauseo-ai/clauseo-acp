// Clauseo Case Law, at the head of Connectors: its card and the blocks its detail adds, by
// case-law-model.ts's caseLawShown. Not added, the card offers it in the agreed words (shared with
// the home card, UC-1090 to UC-1095) and its one button opens today's add flow, Settings at
// Connectors. Added, it shows the credits left. Out of reach, it keeps its name and line, says so
// with a yellow dot, and offers Retry (the list read again checks again). Not offered yet, it says
// so with no button. With
// connectors turned off it says so and opens Settings; before the connectors' state arrives it
// waits, so a lawyer who holds credits is never offered them again.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { KeychainAllow } from "../room/first-run/keychain-pointer.tsx";
import type { Keychain } from "../room/first-run/keychain-state.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import type { CaseLawFacts, Recheck } from "./addons-source.ts";
import { caseLawShown, creditLines, priceLines } from "./case-law-model.ts";
import { ItemTile } from "./glyphs.tsx";
import { descriptionOf, nameOf, scopeTag, troubleTag } from "./model.ts";

export type CaseLawActs = {
  facts: CaseLawFacts;
  /** Today's add flow and the connectors' switch, Settings at Connectors; null without it. */
  openConnectors: (() => void) | null;
  /** Its reach checked again; null where it can't be. */
  recheck: Recheck | null;
  /** Add credits: the host's checked pay link through the app's link path when out of credit with one, else
   * `openConnectors`; null without either. */
  addCredits: (() => void) | null;
  /** The keychain row's pointer while it needs allowing: a signed-out Case Law or a tool's missing key may be only
   * unreadable, so they point at the row's Allow, never at a sign-in or the key typed again. */
  keychain?: Keychain | null | undefined;
};

type Names = ReadonlyMap<MatterId, string>;

/** Retry: the list read again, which checks Case Law's reach again. */
export function RecheckButton({ recheck, small }: { recheck: Recheck | null; small?: boolean }) {
  const { lang } = useDisplay();
  if (recheck === null) return null;
  return (
    <Button
      kind="tone"
      {...(small === true ? { size: "small" as const } : { className: "lib-feature-add" })}
      onClick={recheck.run}
    >
      {lb(lang, "Retry")}
    </Button>
  );
}

function Tag({ text }: { text: string }) {
  return (
    <span className="lib-feature-meta">
      <span className="lib-row-tag" data-trouble="" data-testid="case-law-trouble">
        {text}
      </span>
    </span>
  );
}

function Meta({ a, acts, names }: { a: AddonView; acts: CaseLawActs; names: Names }) {
  const { lang } = useDisplay();
  const shown = caseLawShown(a, acts.facts);
  if (shown === "waiting") return null;
  if (shown === "offer") {
    const lines = priceLines(lang, acts.facts.pack);
    return (
      <span className="lib-feature-meta lib-feature-price">
        {lines.map((line, i) => (
          <span key={line} className={i === 0 && lines.length > 1 ? "lib-credits" : undefined}>
            {line}
          </span>
        ))}
      </span>
    );
  }
  if (shown === "unreachable") return <Tag text={lb(lang, "Can't reach it right now")} />;
  if (shown === "unavailable") return <Tag text={lb(lang, "Not available yet.")} />;
  if (shown === "off") return <Tag text={lb(lang, "Turned off")} />;
  if (shown === "ended")
    return (
      <Tag
        text={acts.keychain ? fl(lang, "Needs your Mac's permission") : lb(lang, "Signed out")}
      />
    );
  if (shown === "trouble")
    return <Tag text={troubleTag(lang, acts.facts.off === "firm" ? "blocked" : a.status) ?? ""} />;
  const credits = acts.facts.credits;
  const out = acts.facts.out !== null;
  return (
    <span className="lib-feature-meta">
      {(credits !== null || out) &&
        creditLines(lang, credits, out).map((line, i) => (
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
  const shown = caseLawShown(a, acts.facts);
  const open = acts.openConnectors;
  const action =
    shown === "ended" && acts.keychain ? (
      <KeychainAllow k={acts.keychain} small={false} />
    ) : shown === "offer" && open !== null ? (
      <Button kind="primary" className="lib-feature-add" onClick={open}>
        {cr(lang, "Add Clauseo Case Law")}
      </Button>
    ) : (shown === "off" || shown === "ended") && open !== null ? (
      <Button kind="tone" className="lib-feature-add" onClick={open}>
        {lb(lang, "Open Settings")}
      </Button>
    ) : shown === "unreachable" ? (
      <RecheckButton recheck={acts.recheck} />
    ) : null;
  return (
    <div
      className="lib-feature"
      data-act={action === null ? undefined : ""}
      data-testid="case-law-card"
      data-shown={shown}
      aria-busy={shown === "waiting" ? true : undefined}
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
 * again; added, its credits and Add credits (today's flow, in Settings); turned off, how to turn
 * connectors on. */
export function CaseLawBlocks({ a, acts, who }: { a: AddonView; acts: CaseLawActs; who: string }) {
  const { lang } = useDisplay();
  const shown = caseLawShown(a, acts.facts);
  const open = acts.openConnectors;
  const cost = lb(
    lang,
    "A page of search results is 20 credits. Opening a judgment is 3 credits, plus 1 for each 4,000 tokens {who} reads (about 3,000 words).",
    { who },
  );
  if (shown === "off")
    return (
      <div className="lib-trouble" role="status">
        <p>
          {lb(
            lang,
            "Connectors are turned off. You can turn them on in Settings, under Connectors.",
          )}
        </p>
        {open !== null && (
          <Button kind="tone" size="small" onClick={open}>
            {lb(lang, "Open Settings")}
          </Button>
        )}
      </div>
    );
  if (shown === "ended" && acts.keychain)
    return (
      <div className="lib-trouble" role="status">
        <p>{acts.keychain.line}</p>
        <KeychainAllow k={acts.keychain} />
      </div>
    );
  if (shown === "ended")
    return (
      <div className="lib-trouble" role="status">
        <p>{lb(lang, "Sign in again in Settings, under Connectors.")}</p>
        {open !== null && (
          <Button kind="tone" size="small" onClick={open}>
            {lb(lang, "Open Settings")}
          </Button>
        )}
      </div>
    );
  if (shown === "offer") {
    const price = priceLines(lang, acts.facts.pack);
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
        <p className="text-ui text-primary">{price.join(" ")}</p>
        <p className="text-small text-muted">{cost}</p>
        {open !== null && (
          <div className="lib-detail-acts">
            <Button kind="primary" onClick={open}>
              {cr(lang, "Add Clauseo Case Law")}
            </Button>
          </div>
        )}
      </section>
    );
  }
  if (shown !== "added") return null;
  const credits = acts.facts.credits;
  const out = acts.facts.out !== null;
  const add = acts.addCredits;
  return (
    <section className="lib-detail-block">
      {(credits !== null || out) && (
        <p className="text-ui text-primary" data-testid="case-law-credits">
          <span className="lib-credits">{creditLines(lang, credits, out).join(" · ")}</span>
        </p>
      )}
      <p className="text-small text-muted">{cost}</p>
      {add !== null && (
        <div className="lib-detail-acts">
          <Button
            kind={out ? "primary" : "tone"}
            size="small"
            data-testid="case-law-add-credits"
            onClick={add}
          >
            {cr(lang, "Add credits")}
          </Button>
        </div>
      )}
    </section>
  );
}
