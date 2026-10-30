// The draft in review. The draft as Word holds it, with every tracked change marked where it stands: words that came
// in underlined, words that went out struck, and the lawyer's own at 600, because weight is authorship. "Open in Word"
// is one click away, and while Word holds the file, accepting waits for it to close. Its changes and its citations
// stand beside it, in draft-pane.tsx.
import { useEffect, useLayoutEffect, useRef, type ReactNode, type RefObject } from "react";
import { Dialog as Base } from "@base-ui/react/dialog";
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { Kbd } from "../ui/kbd.tsx";
import {
  dialogActions,
  dialogBody,
  dialogPlace,
  dialogPlane,
  dialogTitle,
  scrim,
} from "../ui/recipes-overlays.ts";
import { IconBackToAnswer, IconOpen } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useReaderWords } from "../lib/words-reader.ts";
import { quietStart } from "../ui/dialog-start.ts";
import { Heading } from "./heading.tsx";
import {
  changesIn,
  draftCiteIn,
  draftRuns,
  standingParas,
  type ChangeId,
  type CheckStatus,
  type Draft,
  type DraftCite,
  type DraftRun,
  type Review,
} from "./judgments.ts";
import { keepIds } from "./matter.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";

/* ---------- the sheet ---------- */

/** A citation as the draft writes it. While the citations are in view it carries its check's status. */
function CiteWords({
  c,
  status,
  focused,
  onFocus,
}: {
  c: DraftCite | undefined;
  status: CheckStatus | "checking" | null;
  focused: boolean;
  onFocus?: ((id: string) => void) | undefined;
}) {
  if (c === undefined) return null;
  return (
    <span
      className="rd-cite"
      lang="en"
      data-status={status ?? undefined}
      data-focus={focused ? "" : undefined}
      onClick={onFocus === undefined ? undefined : () => onFocus(`cite:${c.id}`)}
    >
      {keepIds(c.text)}
    </span>
  );
}

export function Inner({
  runs,
  cites,
}: {
  runs: Extract<DraftRun, { kind: "ins" | "del" }>["runs"];
  cites: (id: string) => ReactNode;
}) {
  return (
    <>
      {runs.map((r, i) =>
        r.kind === "text" ? (
          <Say key={i}>{keepIds(r.text)}</Say>
        ) : (
          <span key={i}>{cites(r.id)}</span>
        ),
      )}
    </>
  );
}

export function DraftSheet({
  draft,
  review,
  tab,
  onFocus,
}: {
  draft: Draft;
  review: Review;
  tab: "changes" | "citations";
  onFocus: (id: string) => void;
}) {
  const w = useReaderWords();
  const { id } = useRoom();
  const shown = new Set(changesIn(review, draft.changes).map((c) => c.id));
  const by = new Map(draft.changes.map((c) => [c.id, c.by]));
  const statusOf = (cid: string) =>
    tab === "citations" ? (review.statuses[cid] ?? draftCiteIn(draft, cid)?.status ?? null) : null;
  const cites = (cid: string) => (
    <CiteWords
      c={draftCiteIn(draft, cid)}
      status={statusOf(cid)}
      focused={tab === "citations" && review.focus === `cite:${cid}`}
      onFocus={tab === "citations" ? onFocus : undefined}
    />
  );
  const stateOf = (c: ChangeId) => review.changes[c]?.state ?? "pending";
  // a whole paragraph that came in with a change the lawyer can't see yet, or rejected, drops out, and the rest close
  // up
  const gone = (c: ChangeId) => !shown.has(c) || stateOf(c) === "rejected";
  return (
    <article
      className="page rd-page rd-draft"
      aria-labelledby={id("rd-draft-title")}
      data-tab={tab}
    >
      <p className="page-court">
        <Say>{draft.court}</Say>
      </p>
      <p className="page-case">
        <Say>{draft.caseLine}</Say>
      </p>
      <div className="page-parties">
        {draft.parties.map((p, i) => (
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
      <Heading level={2} className="page-title" id={id("rd-draft-title")}>
        <Say>{draft.heading}</Say>
      </Heading>
      <div className="page-paras">
        {standingParas(draft.paras, gone).map((text, pi) => {
          if (text === null) return null;
          return (
            <p key={pi}>
              {draftRuns(text).map((r, i) => {
                if (r.kind === "same") return <Say key={i}>{keepIds(r.text)}</Say>;
                if (r.kind === "cite") return <span key={i}>{cites(r.id)}</span>;
                if (!shown.has(r.change)) return null;
                const st = stateOf(r.change);
                const mine = by.get(r.change) === "you";
                const focus = review.focus === r.change ? "" : undefined;
                const words = <Inner runs={r.runs} cites={cites} />;
                if (r.kind === "ins") {
                  if (st === "rejected") return null;
                  if (st === "accepted")
                    return (
                      <span key={i} className="rd-acc">
                        {words}
                      </span>
                    );
                  return (
                    <ins
                      key={i}
                      className="rd-ins"
                      data-by={mine ? "you" : "clauseo"}
                      data-focus={focus}
                      onClick={() => onFocus(r.change)}
                    >
                      {words}
                    </ins>
                  );
                }
                if (st === "accepted") return null;
                if (st === "rejected") return <span key={i}>{words}</span>;
                return (
                  <del
                    key={i}
                    className="rd-del"
                    data-focus={focus}
                    onClick={() => onFocus(r.change)}
                  >
                    {words}
                  </del>
                );
              })}
            </p>
          );
        })}
      </div>
      <p className="page-foot">
        <span aria-hidden="true">1</span>
        <span className="sr-only">{w.t("Page 1")}</span>
      </p>
    </article>
  );
}

/** The draft over the thread: the bar with Back, the Prepared chip, the draft's title, its page, and Open in Word,
 * which says since when Word has had it; then the sheet on the desk. */
export function DraftView({
  draft,
  review,
  tab,
  wordSince,
  backRef,
  onBack,
  onOpenInWord,
  onFocus,
}: {
  draft: Draft;
  review: Review;
  tab: "changes" | "citations";
  /** Since when Word has had the file open, or null while it hasn't. */
  wordSince: string | null;
  backRef: RefObject<HTMLButtonElement | null>;
  onBack: () => void;
  onOpenInWord: () => void;
  onFocus: (id: string) => void;
}) {
  const w = useReaderWords();
  const desk = useRef<HTMLDivElement>(null);
  // it opens at the paragraphs, where the changes are, with the parties just out of view
  useLayoutEffect(() => {
    const d = desk.current;
    const paras = d?.querySelector<HTMLElement>(".page-paras");
    if (d == null || paras == null) return;
    d.scrollTop = Math.max(
      0,
      paras.getBoundingClientRect().top - d.getBoundingClientRect().top - 120,
    );
  }, []);
  // the change or citation the lawyer points at beside the sheet comes into view on it
  useEffect(() => {
    const marks = desk.current?.querySelectorAll<HTMLElement>("ins, del, .rd-cite") ?? [];
    const el = [...marks].find((m) => m.hasAttribute("data-focus"));
    el?.scrollIntoView({ block: "nearest" });
  }, [review.focus, tab]);
  return (
    <section className="rd-reader rd-draft-view" role="group" aria-label={draft.title}>
      <div className="rd-bar">
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
        <div className="rd-what">
          <Chip role="prepared" />
          <b>
            <Say>{draft.title}</Say>
          </b>
        </div>
        {wordSince === null ? (
          <Button kind="tone" size="small" className="rd-word" onClick={onOpenInWord}>
            {w.r("Open in Word")}
            <IconOpen size={14} />
          </Button>
        ) : (
          <span className="rd-word-since">
            <Say>{w.r("Open in Word since {time}", { time: w.clock(wordSince) })}</Say>
          </span>
        )}
      </div>
      <div className="rd-desk-wrap">
        <div className="rd-desk" ref={desk} tabIndex={0} role="region" aria-label={draft.title}>
          <DraftSheet draft={draft} review={review} tab={tab} onFocus={onFocus} />
        </div>
      </div>
    </section>
  );
}

/** Word holds the file: the same words Versions uses for restoring, with Try again once it's closed. */
export function WordLockDialog({
  file,
  open,
  act,
  onClose,
  onTryAgain,
}: {
  /** The Word file's name. */
  file: string;
  open: boolean;
  act: "accept" | "reject";
  onClose: () => void;
  onTryAgain: () => void;
}) {
  const w = useReaderWords();
  const cancel = useRef<HTMLButtonElement>(null);
  return (
    <Base.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Base.Portal>
        <Base.Backdrop className={scrim} />
        <Base.Popup
          ref={quietStart}
          data-dialog-plane=""
          className={cn(dialogPlace, dialogPlane, "rd-dialog")}
          initialFocus={cancel}
        >
          <Base.Title className={dialogTitle}>
            <Say>
              {w.r(
                act === "accept"
                  ? "Close '{file}' in Word to accept these changes."
                  : "Close '{file}' in Word to reject these changes.",
                { file },
              )}
            </Say>
          </Base.Title>
          <Base.Description className={dialogBody}>
            <Say>{w.r("Word holds the file while it is open. Nothing has changed yet.")}</Say>
          </Base.Description>
          <div className={dialogActions}>
            <Base.Close render={<Button kind="quiet" ref={cancel} />}>{w.ui("Cancel")}</Base.Close>
            <Button kind="tone" className="rd-try" onClick={onTryAgain}>
              {w.r("Try again")}
            </Button>
          </div>
        </Base.Popup>
      </Base.Portal>
    </Base.Root>
  );
}
