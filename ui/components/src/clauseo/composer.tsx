import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { Button } from "../ui/button.tsx";
import {
  IconAddPapers,
  IconNewMatter,
  IconPaperBookPage,
  IconSafeWorkspace,
} from "../icons/icons.tsx";
import { motionAllowed, useDisplay } from "../lib/display.ts";
import { useThreadWords } from "../lib/words-thread.ts";
import { rt } from "../lib/words-room.ts";
import { pbWords, type Paper } from "./matter.ts";
import { dur, ease } from "./motion.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";
import { StatusLine, type PlanChoice, type ReadyHint } from "./status-line.tsx";
import { WorkingLine, WorkingTime, type WorkingState } from "./working-line.tsx";

export type { PlanChoice, ReadyHint } from "./status-line.tsx";

/** A paper added to the question, as a small raised pill on the field's well: the paper book page glyph, its short
 * title and page, and a press to take it off again. */
export type QuestionPaper = Pick<Paper, "id" | "title" | "pb">;

function PaperChips({
  papers,
  onRemove,
}: {
  papers: readonly QuestionPaper[];
  onRemove?: ((id: string) => void) | undefined;
}) {
  const w = useThreadWords();
  return (
    <ul className="q-papers" aria-label={w.th("Papers for this question")}>
      {papers.map((p) => (
        <li key={p.id} className="q-paper">
          <IconPaperBookPage size={14} className="q-paper-icon" />
          <span className="q-paper-title">
            <Say>{p.title}</Say>
          </span>
          <span className="q-paper-pb">{pbWords(w.lang, p.pb)}</span>
          <button
            type="button"
            className="q-paper-remove"
            aria-label={w.th("Remove {title}", { title: p.title })}
            onClick={() => onRemove?.(p.id)}
          >
            <IconNewMatter size={14} className="rotate-45" />
          </button>
        </li>
      ))}
    </ul>
  );
}

/** The working line opens upward from the field in 240 ms on the spring: the well grows from the field's own
 * height to take the row, whose words are already on it. Once, as Clauseo starts; its words change in place after. */
function useOpening(ref: RefObject<HTMLDivElement | null>, on: boolean) {
  useLayoutEffect(() => {
    const node = ref.current;
    const row = node?.firstElementChild;
    if (node == null || !(row instanceof HTMLElement) || !on || !motionAllowed()) return;
    const r = getComputedStyle(node).borderTopLeftRadius;
    const h = Math.round(row.offsetHeight);
    node.animate(
      [{ clipPath: `inset(${h}px 0 0 0 round ${r})` }, { clipPath: `inset(0 0 0 0 round ${r})` }],
      { duration: dur("grow"), easing: ease("spring") },
    );
    row.animate([{ transform: "translateY(14px)" }, { transform: "none" }], {
      duration: dur("grow"),
      easing: ease("spring"),
    });
  }, [ref, on]);
}

/** Whether a press in the field landed on a button or the text itself, which take it themselves. */
function pressOwn(target: EventTarget, field: Element): boolean {
  for (
    let n = target instanceof Element ? target : null;
    n !== null && n !== field;
    n = n.parentElement
  ) {
    if (n instanceof HTMLButtonElement || n instanceof HTMLTextAreaElement) return true;
  }
  return false;
}

export type ComposerProps<V extends string = string> = {
  /** "Took 38 seconds" once an answer is done, or nothing before the first. */
  took: string | null;
  plan: string;
  hint: ReadyHint | null;
  /** While Clauseo works, or while an error or a wait stands: the working line, in the status line's place. */
  working?: WorkingState | null;
  /** Whether the working line opens with its motion, as it does when Clauseo starts. */
  opening?: boolean;
  /** Clauseo is answering: Stop stands where Ask stood. */
  busy?: boolean;
  onStop?: () => void;
  /** The lawyer's words, when the room holds them; otherwise the field keeps its own. */
  text?: string;
  onText?: (text: string) => void;
  papers?: readonly QuestionPaper[];
  onRemovePaper?: (id: string) => void;
  choice?: PlanChoice<V>;
  /** Keys the field passes on first, such as the permission prompt's. True when one was taken. */
  onKeyCommand?: (e: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  /** The field's id, unique on the page. */
  id?: string;
  /** The one act left before the field can take a question, such as signing in to a plan: it stands in the field's
   * row in place of the words, Add papers and Ask, on the same well. */
  gate?: ReactNode;
  /** Words in the room's line's place, such as the fine print before signing in. */
  line?: ReactNode;
  /** Questions drawn from the matter's papers, offered over an empty field: a press puts one in the field. */
  prompts?: { label: string; items: readonly { text: string; onPick: () => void }[] };
  /** A quiet note under the field, above the room's line, such as the plan running low. Optional. */
  notice?: ReactNode;
  /** What waits above the composer, such as the messages queued while Clauseo works. */
  queue?: ReactNode;
  /** The line above the field, with its own acts at its right end, such as what Send now does with Send now and Queue
   * while Clauseo works. The field's row holds only Add papers and Ask, or Stop in Ask's place, so the field never
   * changes width. */
  acts?: ReactNode;
  /** The field's placeholder and name, where the room isn't a matter's: the scratch matter's "Ask a quick question". */
  placeholder?: string;
  /** Ask, pressed, or the form sent. */
  onAsk?: () => void;
};

/** The composer: the status line or the working line, then a field set into the work's surface, Add papers as a
 * quiet icon button, the lawyer's own words at 17 on 26 at 600, and Ask, or Stop while Clauseo works; then the
 * room's line under it, with the lock. */
export function Composer<V extends string = string>({
  took,
  plan,
  hint,
  working = null,
  opening = false,
  busy = false,
  onStop,
  text: held,
  onText,
  papers = [],
  onRemovePaper,
  choice,
  onKeyCommand,
  id = "ask",
  gate,
  line,
  prompts,
  notice,
  queue,
  acts,
  placeholder,
  onAsk,
}: ComposerProps<V>) {
  const w = useThreadWords();
  const [own, setOwn] = useState("");
  const text = held ?? own;
  const setText = onText ?? setOwn;
  const area = useRef<HTMLTextAreaElement>(null);
  const entry = useRef<HTMLDivElement>(null);
  const room = useRoom();
  const fieldId = room.id(id);
  const isWorking = working !== null;
  useOpening(entry, isWorking && opening);
  // the field takes a second line, and more, when what it holds needs one, the placeholder too: on a narrow window or
  // at a large text size the words wrap rather than hide. At one line it keeps the stylesheet's own height.
  const { lang, textSize } = useDisplay();
  useLayoutEffect(() => {
    const a = area.current;
    if (a === null) return;
    const fit = () => {
      a.style.height = "";
      const one = a.clientHeight;
      if (a.scrollHeight > one + 4) a.style.height = `${Math.min(a.scrollHeight, one * 6)}px`;
    };
    fit();
    let width = a.clientWidth;
    const ro = new ResizeObserver(() => {
      if (a.clientWidth === width) return;
      width = a.clientWidth;
      fit();
    });
    ro.observe(a);
    return () => ro.disconnect();
  }, [text, lang, textSize]);
  // the room keeps the composer's height, so what scrolls into view on a phone stands above it, and the turned page
  // stands clear of it
  useEffect(() => {
    const form = room.nodes.get("composer");
    const win = room.nodes.get("window");
    if (form === null || win === null) return;
    let h = -1;
    const ro = new ResizeObserver(() => {
      const next = Math.round(form.offsetHeight);
      if (next === h) return;
      h = next;
      win.style.setProperty("--composer-h", `${next}px`);
    });
    ro.observe(form);
    return () => ro.disconnect();
  }, [room]);
  return (
    <form
      ref={room.nodes.slot("composer")}
      className="room-composer"
      data-composer=""
      onSubmit={(e) => {
        e.preventDefault();
        onAsk?.();
      }}
    >
      {queue}
      {prompts !== undefined &&
        prompts.items.length > 0 &&
        text.trim() === "" &&
        gate === undefined && (
          <ul className="room-prompts" aria-label={prompts.label}>
            {prompts.items.map((p) => (
              <li key={p.text}>
                <button type="button" className="room-prompt" onClick={p.onPick}>
                  <Say>{p.text}</Say>
                </button>
              </li>
            ))}
          </ul>
        )}
      {acts}
      <StatusLine took={took} plan={plan} hint={hint} choice={choice} hidden={working !== null} />
      {/* while Clauseo works the working line is the well's top row, and the field its foot: one set-in region */}
      <div className="room-entry" ref={entry} data-working={working !== null ? "" : undefined}>
        {working !== null && <WorkingLine state={working} />}
        <div
          className="room-field"
          data-papers={papers.length > 0 ? "" : undefined}
          data-gate={gate !== undefined ? "" : undefined}
          onPointerDown={(e) => {
            // the whole field takes the pointer, not just its one line of text
            if (gate !== undefined || pressOwn(e.target, e.currentTarget)) return;
            e.preventDefault();
            area.current?.focus();
          }}
        >
          {gate !== undefined ? (
            gate
          ) : (
            <>
              {papers.length > 0 && <PaperChips papers={papers} onRemove={onRemovePaper} />}
              <label className="sr-only" htmlFor={fieldId}>
                {placeholder ?? w.t("Ask about this matter")}
              </label>
              <textarea
                id={fieldId}
                ref={(node) => {
                  area.current = node;
                  return room.nodes.slot("field")(node);
                }}
                rows={1}
                value={text}
                placeholder={placeholder ?? w.t("Ask about this matter")}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (onKeyCommand?.(e) === true) e.preventDefault();
                }}
              />
              <Button
                kind="quiet"
                shape="icon"
                className="room-field-add"
                aria-label={w.t("Add papers")}
              >
                <IconAddPapers size={16} />
              </Button>
              {working !== null && <WorkingTime state={working} />}
              {busy ? (
                <Button
                  kind="tone"
                  className="room-field-ask"
                  aria-label={w.th("Stop the answer")}
                  onClick={onStop}
                >
                  <span className="deva-nudge">{w.t("Stop")}</span>
                </Button>
              ) : (
                <Button
                  kind="primary"
                  type="submit"
                  className="room-field-ask"
                  disabled={text.trim() === ""}
                >
                  {w.t("Ask")}
                </Button>
              )}
            </>
          )}
        </div>
      </div>
      {notice}
      <p className="room-line">
        {line ?? (
          <>
            <IconSafeWorkspace size={14} />
            <span>
              <Say>
                {rt(
                  w.lang,
                  "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.",
                )}
              </Say>
            </span>
          </>
        )}
      </p>
    </form>
  );
}
