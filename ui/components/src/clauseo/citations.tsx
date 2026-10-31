import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FocusEvent,
  type MouseEvent,
  type PointerEvent,
} from "react";
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { CollapsiblePanel, CollapsibleTrigger } from "../ui/collapsible.tsx";
import { useDisplay } from "../lib/display.ts";
import { useWords } from "../lib/words.ts";
import { rt } from "../lib/words-room.ts";
import { useCarded, useHandState, useInHand, type Target } from "./hand.ts";
import { isWide, useWide } from "./layout.ts";
import { fitCitations } from "./logic.ts";
import { BENCH_LABEL, benchOf, benchWords, keepIds, type Bench, type Cite } from "./matter.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";
import { Heading } from "./heading.tsx";

/** The bench mark: one filled circle, sized by the bench on the fixed scale; primary for us, red for them. */
export function BenchMark({ bench, stance }: { bench: Bench; stance: "for" | "against" }) {
  return <span className="bench" data-bench={bench} data-stance={stance} aria-hidden="true" />;
}

export type CitationRowProps = {
  id: string;
  cite: Cite;
  /** Whether its check against later decisions has finished. Until then it says Checking, with what it reads. */
  checked?: boolean | undefined;
};

/** One judgment the answer cites, in two lines: the bench mark, its short name underlined, and For us or They will
 * cite; then Good law with its tick and the citation, pin and bench. Pointing at it or focusing its name shows its
 * card; a click, a tap or Enter puts it in hand. */
export function CitationRow({ id, cite, checked = true }: CitationRowProps) {
  const w = useWords();
  const wide = useWide();
  const room = useRoom();
  const { hand, nodes } = room;
  const target: Target = { kind: "cite", id };
  const inHand = useInHand(hand, target);
  const carded = useCarded(hand, target, wide);
  const bench = benchOf(cite.forum, cite.bench);
  const stance = cite.stance === "against" ? "against" : "for";
  const where = [cite.cite, cite.pin, benchWords(w.lang, bench)]
    .filter((s) => s !== null)
    .join(", ");

  const onPointerEnter = (e: PointerEvent<HTMLDivElement>) => {
    if (e.pointerType !== "touch") hand.point(e.currentTarget, target, "row");
  };
  const onPointerLeave = (e: PointerEvent<HTMLDivElement>) => {
    if (e.pointerType !== "touch") hand.leave();
  };
  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    // the name is a link for the keyboard and for reading; a press on it is the row's
    if (nodes.find("link", id)?.contains(e.target as Node) === true) e.preventDefault();
    hand.press(e.currentTarget, target, "row", e.detail === 0);
  };
  const onFocus = (e: FocusEvent<HTMLAnchorElement>) => {
    const row = nodes.find("row", id);
    if (row !== null && e.currentTarget.matches(":focus-visible")) hand.focusOn(row, target, "row");
  };

  return (
    // the row moves when in hand; its place in the list doesn't, so a card measures the place, not the row
    <li className="cite" id={room.id(`cite-${id}`)} ref={nodes.keep("cite", id)}>
      <div
        ref={nodes.keep("row", id, true)}
        className="cite-row card-trigger"
        data-card={`cite:${id}`}
        data-hand={inHand ? "" : undefined}
        data-carded={carded ? "" : undefined}
        onPointerEnter={onPointerEnter}
        onPointerLeave={onPointerLeave}
        onClick={onClick}
      >
        <span
          className="cite-mark"
          title={w.t("{bench}. Mark size follows the bench.", { bench: w.t(BENCH_LABEL[bench]) })}
        >
          <BenchMark bench={bench} stance={stance} />
        </span>
        <p className="cite-name">
          <a
            ref={nodes.keep("link", id)}
            href={`#${room.id(`cite-${id}`)}`}
            lang="en"
            className="as-en"
            aria-label={w.t("{name}, {cite}", { name: cite.name, cite: cite.cite })}
            onFocus={onFocus}
            onBlur={hand.leave}
          >
            {cite.short}
          </a>
        </p>
        <Chip role={stance === "against" ? "cite" : "for"} className="cite-stance" />
        {checked ? (
          <p className="cite-check" data-state="done" ref={nodes.keep("check", id)}>
            <Chip role="good" icon="tick">
              {cite.check.result}
            </Chip>
            <span className="cite-where">
              <Say>{keepIds(where)}</Say>
            </span>
          </p>
        ) : (
          <p className="cite-check" data-state="wait">
            <Chip role="checking" />
            <span className="cite-wait">
              <Say>{cite.check.waitingFor}</Say>
            </span>
          </p>
        )}
      </div>
    </li>
  );
}

/* ---------- the citations, whole rows only ---------- */

export type CitationsProps = {
  cites: readonly { id: string; cite: Cite; checked?: boolean }[];
  /** Anything else on the wall that moves the rows, such as the prepared draft's tile arriving above them. */
  layout?: string;
};

/** "Citations 4", "Copy list", and the rows. The wall shows whole rows only: when some don't fit, a 40 px strip in
 * the ground's tone stands under the last whole row and says "N more", and opens to the rest. */
export function Citations({ cites, layout = "" }: CitationsProps) {
  const w = useWords();
  const { lang, textSize } = useDisplay();
  const wide = useWide();
  const listRef = useRef<HTMLOListElement>(null);
  const [paneSize, setPaneSize] = useState("");
  // the fit holds for one set of words, size and window; for any other, every row is laid out and measured again
  const key = `${lang}|${textSize}|${wide}|${paneSize}|${cites.length}|${layout}`;
  const [fit, setFit] = useState<{ key: string; shown: number } | null>(null);
  const measuring = fit === null || fit.key !== key;
  const [open, setOpen] = useState(false);
  const { id, nodes, hand: store } = useRoom();
  const hand = useHandState(store);

  useLayoutEffect(() => {
    if (!measuring) return;
    const list = listRef.current;
    const pane = nodes.get("pane");
    let shown = cites.length;
    if (list != null && pane != null && isWide()) {
      const pr = pane.getBoundingClientRect();
      const rows = [...list.children].map((r) => r.getBoundingClientRect());
      const strip = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) * 2.5;
      shown = fitCitations(rows, pr.top, pr.bottom, strip).shown;
    }
    // every row stands whole: the measuring render is already right, so it stays, and costs no second commit. That
    // holds the live room to one commit a frame as each judgment arrives.
    if (shown === cites.length) return;
    // measuring layout: the rows are laid out first, then as many as stand whole stay, before the frame paints
    // oxlint-disable-next-line react/set-state-in-effect
    setFit({ key, shown });
  }, [measuring, key, cites.length, nodes]);
  useEffect(() => {
    const pane = nodes.get("pane");
    if (pane === null) return;
    const ro = new ResizeObserver(() => setPaneSize(`${pane.clientWidth}x${pane.clientHeight}`));
    ro.observe(pane);
    return () => ro.disconnect();
  }, [nodes]);

  const count = measuring ? cites.length : fit.shown;
  const visible = cites.slice(0, count);
  const rest = cites.slice(count);
  // a row asked for from the answer comes into view first, so its card grows out of the row itself
  const asked =
    hand.state !== "none" &&
    hand.target.kind === "cite" &&
    hand.source === "ref" &&
    rest.some((c) => c.id === hand.target.id);
  const isOpen = open || asked;

  const copy = () => {
    const text = cites.map(({ cite }) => `${cite.name}, ${cite.cite}`).join("\n");
    void navigator.clipboard?.writeText(text).catch(() => {});
  };

  return (
    <section className="flex flex-col" aria-labelledby={id("cites-head")}>
      <header className="wall-head" ref={nodes.keep("head", "cites")}>
        <Heading
          level={2}
          className="wall-title"
          id={id("cites-head")}
          title={w.t("Mark size follows the bench")}
        >
          {w.t("Citations")} <span className="wall-count">{cites.length}</span>
        </Heading>
        <Button
          kind="quiet"
          size="small"
          className="wall-action"
          disabled={cites.length === 0}
          onClick={copy}
        >
          {w.t("Copy list")}
        </Button>
      </header>
      {cites.length === 0 && <p className="cite-empty">{w.t("None cited yet.")}</p>}
      <ol className="cite-list" ref={listRef}>
        {visible.map(({ id, cite, checked }) => (
          <CitationRow key={id} id={id} cite={cite} checked={checked} />
        ))}
      </ol>
      {rest.length > 0 && (
        <BaseCollapsible.Root
          open={isOpen}
          onOpenChange={setOpen}
          // Escape folds the rest away again and puts focus back on "N more", from the trigger or a row it opened
          onKeyDown={(e) => {
            if (e.key !== "Escape" || !isOpen || e.defaultPrevented) return;
            e.stopPropagation();
            setOpen(false);
            e.currentTarget.querySelector<HTMLButtonElement>(".cite-more")?.focus();
          }}
        >
          <CollapsiblePanel>
            <ol className="cite-list mt-s0-5">
              {rest.map(({ id, cite, checked }) => (
                <CitationRow key={id} id={id} cite={cite} checked={checked} />
              ))}
            </ol>
          </CollapsiblePanel>
          <CollapsibleTrigger kind="more" className="cite-more" ref={nodes.slot("citeMore")}>
            {isOpen ? rt(lang, "Show fewer") : w.t("{n} more", { n: rest.length })}
          </CollapsibleTrigger>
        </BaseCollapsible.Root>
      )}
    </section>
  );
}
