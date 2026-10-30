import type { PointerEvent, RefObject } from "react";
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { Kbd } from "../ui/kbd.tsx";
import { IconBackToAnswer, IconPageTurn } from "../icons/icons.tsx";
import { useWords } from "../lib/words.ts";
import { useRoom } from "./room-scope.ts";
import { Dot, Say } from "./say.tsx";
import { Heading } from "./heading.tsx";

export type ReadyCardProps = {
  title: string;
  where: string;
  why: string;
  pages: number;
  /** "Not opened" until the lawyer turns to it, then "Opened". */
  state: "unopened" | "opened";
  /** "Not opened" in the language, without its full stop. */
  unopened: string;
  turnRef?: RefObject<HTMLButtonElement | null>;
  onTurn: () => void;
};

/** The prepared draft, ready at the end of the answer and waiting unopened: a yellow icon tile with the page turn,
 * its title at Clauseo's weight, the Prepared chip, "Not opened" and "9 pages", and "Turn to it" with its key cap.
 * Pointing at it or focusing it lifts the draft's tile off the wall and draws one line between them. */
export function ReadyCard({
  title,
  where,
  why,
  pages,
  state,
  unopened,
  turnRef,
  onTurn,
}: ReadyCardProps) {
  const w = useWords();
  const { hand, nodes } = useRoom();
  const on = (e: PointerEvent) => {
    if (e.pointerType !== "touch") hand.holdDraft("card", true);
  };
  return (
    <div
      className="ready"
      ref={nodes.slot("ready")}
      onPointerEnter={on}
      onPointerLeave={() => hand.holdDraft("card", false)}
    >
      <span className="card-icon">
        <IconPageTurn size={18} />
      </span>
      <p className="ready-title" title={w.t("{title}. {where}.", { title, where })}>
        <Say>{title}</Say>
      </p>
      <p className="ready-detail">
        <Chip role="prepared" className="cursor-default" />
        <span className="ready-state">{state === "opened" ? w.t("Opened") : unopened}</span>
        <span className="ready-pages">
          <Dot />
          {w.t("{n} pages", { n: pages })}
        </span>
        <span className="sr-only">{w.t("{why}.", { why })}</span>
      </p>
      <Button
        ref={turnRef}
        kind="tone"
        className="ready-turn gap-s2 pr-s3 pl-s4"
        aria-keyshortcuts="ArrowRight"
        aria-label={w.t("Turn to it: {title}, {where}", { title, where })}
        onClick={onTurn}
        onFocus={(e) => {
          if (e.currentTarget.matches(":focus-visible")) hand.holdDraft("card", true);
        }}
        onBlur={() => hand.holdDraft("card", false)}
      >
        {w.t("Turn to it")}
        <Kbd>→</Kbd>
      </Button>
    </div>
  );
}

type Preview = {
  court: string;
  caseLine: string;
  parties: readonly { name: string; role: string }[];
  title: string;
  paras: readonly string[];
};

export type TurnedPageProps = {
  open: boolean;
  title: string;
  pages: number;
  preview: Preview;
  backRef: RefObject<HTMLButtonElement | null>;
  pageRef: RefObject<HTMLElement | null>;
  turnRef: RefObject<HTMLElement | null>;
  onBack: () => void;
};

/** The draft's first page, turned to: a sheet on the desk tone with the court line, the parties with dotted leaders
 * to their roles, the title, the paragraphs and the page number, under a bar with "Back to the answer" and its key
 * cap, the Prepared chip, the draft's title and "Page 1 of 9". */
export function TurnedPage({
  open,
  title,
  pages,
  preview,
  backRef,
  pageRef,
  turnRef,
  onBack,
}: TurnedPageProps) {
  const w = useWords();
  const { id, nodes } = useRoom();
  return (
    <section className="turn" aria-labelledby={id("turn-title")} hidden={!open} ref={turnRef}>
      <div className="turn-bar">
        <Button
          ref={backRef}
          kind="quiet"
          size="small"
          className="pr-s3 pl-s2"
          aria-keyshortcuts="ArrowLeft"
          onClick={onBack}
        >
          <IconBackToAnswer size={16} />
          {w.t("Back to the answer")}
          <Kbd>←</Kbd>
        </Button>
        <p className="turn-what">
          <Chip role="prepared" />
          <b>
            <Say>{title}</Say>
          </b>
          <span className="turn-page">{w.t("Page 1 of {n}", { n: pages })}</span>
        </p>
      </div>
      {/* focusable, so the page scrolls from the keyboard too */}
      <div
        className="turn-scroll"
        ref={nodes.slot("turnScroll")}
        tabIndex={0}
        role="region"
        aria-label={w.t("{title}, page 1 of {n}", { title, n: pages })}
      >
        <article className="page" ref={pageRef}>
          <p className="page-court">
            <Say>{preview.court}</Say>
          </p>
          <p className="page-case">
            <Say>{preview.caseLine}</Say>
          </p>
          <div className="page-parties">
            {preview.parties.map((p, i) => (
              <div key={p.name} className="contents">
                {i > 0 && <p className="page-versus">{w.t("Versus")}</p>}
                <p className="page-party">
                  <span>
                    <Say>{p.name}</Say>
                  </span>
                  <span className="page-role">{p.role}</span>
                </p>
              </div>
            ))}
          </div>
          <Heading level={2} className="page-title" id={id("turn-title")} tabIndex={-1}>
            <Say>{preview.title}</Say>
          </Heading>
          <div className="page-paras">
            {preview.paras.map((p) => (
              <p key={p.slice(0, 24)}>
                <Say>{p}</Say>
              </p>
            ))}
          </div>
          <p className="page-foot">
            <span aria-hidden="true">1</span>
            <span className="sr-only">{w.t("Page 1")}</span>
          </p>
        </article>
      </div>
    </section>
  );
}
