import type { FocusEvent, KeyboardEvent, MouseEvent, PointerEvent } from "react";
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { useWords } from "../lib/words.ts";
import { useCarded, useDraftHolders, useInHand, type Target } from "./hand.ts";
import { useWide } from "./layout.ts";
import {
  citationsOf,
  citedPapers,
  isLatinOnly,
  pbWords,
  shortTitle,
  type CitedRole,
  type Paper,
  type PaperBookPlace,
  type TileState,
} from "./matter.ts";
import { useMatter } from "./matter-context.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";
import { Heading } from "./heading.tsx";

export type PaperTileProps = {
  /** The paper's id, "dates" for the accepted list of dates, or the prepared draft's id. */
  id: string;
  state: TileState;
  /** What stands where a paper book page would: the paper's pages, "1 to 12", or a word such as "Note", or one of
   * Clauseo's own, "Dates", "Draft". */
  numeral: PaperBookPlace;
  /** The short title, in the language. */
  title: string;
  /** What the tile says beyond its visible words, for assistive technology: its role in words, or what a press
   * does. Its visible numeral, title and chip are its name. */
  label?: string;
  /** A sighted hint, where the room gives one. */
  hint?: string;
  /** Whether a press puts its excerpt card in hand: papers the answer cites, with an excerpt. */
  card?: boolean;
  /** For the prepared draft: a press turns to it. */
  onTurn?: () => void;
  /** Versions: the paper at an earlier time. gone, it wasn't in the matter yet; touched, that version changed it. */
  era?: "gone" | "touched" | undefined;
};

/** One paper on the wall. Uncited it lies flat; cited it fills with its role's colour and stays lit. Hover lifts it
 * 1 px; in hand (pointed at, focused, tapped) it comes forward toward the work; under its card it stays on the wall
 * and its own words step out. */
export function PaperTile({
  id,
  state,
  numeral,
  title,
  label,
  hint,
  card = false,
  onTurn,
  era,
}: PaperTileProps) {
  const w = useWords();
  const wide = useWide();
  const room = useRoom();
  const { hand, nodes } = room;
  const target: Target = { kind: "paper", id };
  const inHand = useInHand(hand, target);
  const carded = useCarded(hand, target, wide);
  const draft = useDraftHolders(hand);
  const lifted = inHand || (state === "prepared" && draft.size > 0);

  const onPointerEnter = (e: PointerEvent<HTMLButtonElement>) => {
    if (e.pointerType === "touch") return;
    if (card) hand.point(e.currentTarget, target, "tile");
    if (state === "prepared") hand.holdDraft("tile", true);
  };
  const onPointerLeave = (e: PointerEvent<HTMLButtonElement>) => {
    if (e.pointerType === "touch") return;
    if (card) hand.leave();
    if (state === "prepared") hand.holdDraft("tile", false);
  };
  const onFocus = (e: FocusEvent<HTMLButtonElement>) => {
    if (card && e.currentTarget.matches(":focus-visible"))
      hand.focusOn(e.currentTarget, target, "tile");
  };
  const onClick = (e: MouseEvent<HTMLButtonElement>) => {
    if (onTurn !== undefined) onTurn();
    else if (card) hand.press(e.currentTarget, target, "tile", e.detail === 0);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (e.key === "Escape" && card) hand.leave();
  };

  const latin = w.lang === "hi" && isLatinOnly(title);
  return (
    <li
      className="tile"
      id={room.id(`paper-${id}`)}
      ref={nodes.keep("tile", id)}
      data-state={state}
      data-hand={lifted ? "" : undefined}
      data-carded={carded ? "" : undefined}
      data-era={era}
      inert={era === "gone" ? true : undefined}
      aria-hidden={era === "gone" ? true : undefined}
    >
      <button
        type="button"
        ref={nodes.keep("face", id, card)}
        className={card ? "tile-face card-trigger" : "tile-face"}
        aria-describedby={label === undefined ? undefined : room.id(`paper-${id}-more`)}
        title={hint}
        data-card={card ? `paper:${id}` : undefined}
        onPointerEnter={onPointerEnter}
        onPointerLeave={onPointerLeave}
        onFocus={onFocus}
        onBlur={card ? hand.leave : undefined}
        onClick={onClick}
        onKeyDown={onKeyDown}
      >
        <span className="tile-no">
          {"word" in numeral ? (
            <span className="tile-num" data-word="">
              {numeral.word}
            </span>
          ) : (
            <>
              <span className="tile-num">{numeral.from}</span>
              {numeral.to !== numeral.from && (
                <span className="tile-to">{w.t("to {n}", { n: numeral.to })}</span>
              )}
            </>
          )}
        </span>
        <span className="tile-title" lang={latin ? "en" : undefined}>
          {latin ? title : <Say>{title}</Say>}
        </span>
        {state !== "uncited" && (
          <span className="tile-foot">
            <Chip
              role={state}
              ground={state === "relied" || state === "accepted" ? "surface" : "tile"}
            />
          </span>
        )}
      </button>
      {label !== undefined && (
        <span className="sr-only" id={room.id(`paper-${id}-more`)}>
          {label}
        </span>
      )}
    </li>
  );
}

/* ---------- the wall of papers ---------- */

/** A paper's tile, in the room's words: lit in its role and carded where the answer cites it. */
function paperTile(p: Paper, role: CitedRole | undefined, t: ReturnType<typeof useWords>) {
  const state: TileState = role ?? "uncited";
  const title = p.title;
  // its name is what it shows; its description, the paper's full title and paper book page
  const label = t.t("{title}, {pb}", { title, pb: pbWords(t.lang, p.pb) });
  return {
    state,
    numeral: p.pb,
    title: shortTitle(title),
    label,
    card: role !== undefined,
  };
}

/** The prepared draft's tile: its id, the short title on the tile, the full title as its hint, and its label. */
export type PreparedTile = { id: string; short: string; title: string; label: string };

export type PapersWallProps = {
  papers: readonly Paper[];
  /** The list of dates the lawyer accepted earlier, where there is one: a matter just opened has none. */
  dates?: { title: string; accepted: string } | null;
  /** The prepared draft, waiting flagged and unopened, once Clauseo has prepared it. */
  draft: PreparedTile | null;
  onTurn: () => void;
};

/** The matter's papers as a facade of tiles in paper book order, under "Papers 14", "By paper book page" and "All
 * 14". Three equal columns; a title that wraps makes its whole row taller rather than clipping. A paper the answer in
 * view cites lights in the role of the citation citedPapers picks for it, read from useMatter() as the card reads it. */
export function PapersWall({ papers, dates = null, draft, onTurn }: PapersWallProps) {
  const w = useWords();
  const matter = useMatter();
  const lit = citedPapers(citationsOf(matter, matter.inView), papers);
  const { id, nodes } = useRoom();
  const n = papers.length;
  return (
    <section className="flex flex-col" aria-labelledby={id("papers-head")}>
      <header className="wall-head" ref={nodes.keep("head", "papers")}>
        <Heading level={2} className="wall-title" id={id("papers-head")}>
          {w.t("Papers")} <span className="wall-count">{n}</span>
        </Heading>
        <p className="wall-sub">{w.t("By paper book page")}</p>
        <Button kind="quiet" size="small" className="wall-action">
          {w.t("All {n}", { n })}
        </Button>
      </header>
      <ol className="facade" ref={nodes.slot("facade")}>
        {papers.map((p) => {
          const tile = paperTile(p, lit.get(p.id)?.citation.role, w);
          return <PaperTile key={p.id} id={p.id} {...tile} />;
        })}
        {dates !== null && (
          <PaperTile
            id="dates"
            state="accepted"
            numeral={{ word: w.t("Dates") }}
            title={dates.title}
            label={dates.accepted}
            hint={w.t("{title}. {accepted}", { title: dates.title, accepted: dates.accepted })}
          />
        )}
        {draft !== null && (
          <PaperTile
            id={draft.id}
            state="prepared"
            numeral={{ word: w.t("Draft") }}
            title={draft.short}
            label={draft.label}
            hint={draft.title}
            onTurn={onTurn}
          />
        )}
      </ol>
    </section>
  );
}
