import type { FocusEvent, KeyboardEvent, MouseEvent, PointerEvent, ReactNode, Ref } from "react";
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { CollapsiblePanel } from "../ui/collapsible.tsx";
import { IconFold, IconGoodLaw } from "../icons/icons.tsx";
import { useWords } from "../lib/words.ts";
import type { Target } from "./hand.ts";
import { isLatinOnly, keepIds, segmentsOf, type Block, type Segment } from "./matter.ts";
import { useMatter } from "./matter-context.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";
import { Heading } from "./heading.tsx";

/** "You, 4:31 pm" or "Clauseo, 4:31 pm": who wrote this, and when. */
export function Who({
  name,
  time,
  ref,
}: {
  name: string;
  time: string;
  ref?: Ref<HTMLParagraphElement>;
}) {
  const w = useWords();
  return (
    <p className="who" ref={ref}>
      <b lang={w.lang === "hi" && isLatinOnly(name) ? "en" : undefined}>{name}</b>, {time}
    </p>
  );
}

/** The lawyer's question in the thread, at the lawyer's weight. */
export function QuestionLine({ who, time, text }: { who: string; time: string; text: string }) {
  const { nodes } = useRoom();
  return (
    <>
      <Who name={who} time={time} ref={nodes.slot("askedWho")} />
      <p className="asked" ref={nodes.slot("asked")}>
        <Say>{text}</Say>
      </p>
    </>
  );
}

/** One thing Clauseo did for an answer, in lawyers' words, with its clock time. */
export type WorkItem = { at: string; what: string };

/** The meta line under the question: "Clauseo, 4:31 pm", then what was checked, as one quiet chip with its chevron
 * that opens a plain list of what Clauseo read, searched and checked, each with its time. */
export function MetaLine({
  who,
  time,
  done,
  read,
}: {
  who: string;
  time: string;
  /** "Checked 4 judgments. 14 papers read." */
  done: string;
  /** What was read, searched and checked, in order. */
  read: readonly WorkItem[];
}) {
  const { nodes } = useRoom();
  return (
    <BaseCollapsible.Root>
      <div className="meta" ref={nodes.slot("meta")}>
        <Who name={who} time={time} />
        <BaseCollapsible.Trigger className="work-done" ref={nodes.slot("workDone")}>
          <IconGoodLaw size={14} className="work-icon" />
          <span>
            <Say>{done}</Say>
          </span>
          <IconFold size={12} className="work-chev" />
        </BaseCollapsible.Trigger>
      </div>
      <CollapsiblePanel>
        <ol className="work-list">
          {read.map((item) => (
            <li key={item.what} className="work-item">
              <span className="work-at">{item.at}</span>
              <span>
                <Say>{item.what}</Say>
              </span>
            </li>
          ))}
        </ol>
      </CollapsiblePanel>
    </BaseCollapsible.Root>
  );
}

/** A reference in the answer's words: underlined in its role's colour, a paper's in green, red or the relied-on
 * grey, a judgment's in the quiet line. Hover, focus or a tap brings what it cites into hand. It never navigates. */
export function Reference({ seg }: { seg: Extract<Segment, { type: "ref" }> }) {
  const w = useWords();
  const room = useRoom();
  const matter = useMatter();
  const { hand, nodes } = room;
  const target: Target = { kind: seg.kind, id: seg.id };
  const cited = seg.kind === "paper" ? matter.cited(seg.id) : undefined;
  const role = cited?.citation.role ?? null;
  const card = seg.kind === "cite" || cited !== undefined;
  const label = keepIds(seg.label);
  const latin = w.lang === "hi" && isLatinOnly(label);

  const onPointerEnter = (e: PointerEvent<HTMLAnchorElement>) => {
    if (card && e.pointerType !== "touch") hand.point(e.currentTarget, target, "ref");
  };
  const onPointerLeave = (e: PointerEvent<HTMLAnchorElement>) => {
    if (card && e.pointerType !== "touch") hand.leave();
  };
  const onFocus = (e: FocusEvent<HTMLAnchorElement>) => {
    if (card && e.currentTarget.matches(":focus-visible"))
      hand.focusOn(e.currentTarget, target, "ref");
  };
  const onClick = (e: MouseEvent<HTMLAnchorElement>) => {
    e.preventDefault();
    if (card) hand.press(e.currentTarget, target, "ref", e.detail === 0);
  };
  // Enter puts the card in hand as the key comes up: on its way down, the focus would move into the card in time for
  // the same key to press Open there
  const onKeyDown = (e: KeyboardEvent<HTMLAnchorElement>) => {
    if (card && e.key === "Enter") e.preventDefault();
  };
  const onKeyUp = (e: KeyboardEvent<HTMLAnchorElement>) => {
    if (card && e.key === "Enter") hand.press(e.currentTarget, target, "ref", true);
  };
  return (
    <a
      ref={card ? nodes.ref(`${seg.kind}:${seg.id}`) : undefined}
      className={card ? "ref card-trigger" : "ref"}
      href={`#${room.id(`${seg.kind === "cite" ? "cite" : "paper"}-${seg.id}`)}`}
      lang={latin ? "en" : undefined}
      data-card={card ? `${seg.kind}:${seg.id}` : undefined}
      data-role={role ?? undefined}
      data-cite={seg.kind === "cite" ? "" : undefined}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
      onFocus={onFocus}
      onBlur={card ? hand.leave : undefined}
      onClick={onClick}
      onKeyDown={onKeyDown}
      onKeyUp={onKeyUp}
    >
      {latin ? label : <Say>{label}</Say>}
    </a>
  );
}

/** A block's words and references, as Clauseo wrote them. */
function Words({ text }: { text: string }) {
  const matter = useMatter();
  return (
    <>
      {segmentsOf(text, matter).map((s, i) =>
        s.type === "ref" ? <Reference key={i} seg={s} /> : <Say key={i}>{keepIds(s.text)}</Say>,
      )}
    </>
  );
}

/** The answer's first line: the largest words on the screen, still at Clauseo's weight. */
export function AnswerLead({ text }: { text: string }) {
  const { nodes } = useRoom();
  return (
    <p className="lead says" ref={nodes.says()}>
      <Words text={text} />
    </p>
  );
}

/** A paragraph of the answer, with its references. */
export function AnswerParagraph({ text }: { text: string }) {
  const { nodes } = useRoom();
  return (
    <p className="para says" ref={nodes.says()}>
      <Words text={text} />
    </p>
  );
}

/** The other side's best case, set apart on its own warmed plane: the red dot and its head, how many judgments they
 * will cite, and the paragraph. */
export function OtherSide({ label, count, text }: { label: string; count: string; text: string }) {
  const { nodes } = useRoom();
  return (
    <section className="otherside" aria-label={label} ref={nodes.slot("otherside")}>
      <header className="os-head">
        <Heading level={3} className="os-label">
          <span className="os-mark" />
          <span>{label}</span>
        </Heading>
        <span className="os-count">{count}</span>
      </header>
      <div className="os-plane">
        <p className="os-text says" ref={nodes.says()}>
          <Words text={text} />
        </p>
      </div>
    </section>
  );
}

/** The answer's blocks in order: the lead, the paragraphs, the other side's case. */
export function AnswerBlocks({
  blocks,
  count,
}: {
  blocks: readonly Block[];
  count: string;
}): ReactNode {
  return blocks.map((b, i) =>
    b.kind === "lead" ? (
      <AnswerLead key={i} text={b.text} />
    ) : b.kind === "otherside" ? (
      <OtherSide key={i} label={b.label ?? ""} count={count} text={b.text} />
    ) : (
      <AnswerParagraph key={i} text={b.text} />
    ),
  );
}
