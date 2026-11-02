import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
  type Ref,
  type RefObject,
} from "react";
import { Button } from "../ui/button.tsx";
import { IconAddPapers, IconSafeWorkspace } from "../icons/icons.tsx";
import { motionAllowed, useDisplay } from "../lib/display.ts";
import { late } from "../lib/late.ts";
import { useThreadWords } from "../lib/words-thread.ts";
import { rt } from "../lib/words-room.ts";
import type { Paper } from "./matter.ts";
import { dur, ease } from "./motion.ts";
import { mayTakeFocus, pressOwn, replaceAll } from "./composer-edit.ts";
import { sendsOnEnter } from "./composer-keys.ts";
import type { MenuShown } from "./composer-menu.tsx";
import { useRoom } from "./room-scope.ts";
import { PaperChips } from "./paper-chips.tsx";
import { Say } from "./say.tsx";
import { StatusLine, type PlanChoice, type ReadyHint } from "./status-line.tsx";
import { WorkingLine, WorkingSaid, WorkingTime, type WorkingState } from "./working-line.tsx";

export type { PlanChoice, ReadyHint } from "./status-line.tsx";

/** One of the agent's commands, as ACP's `available_commands_update` lists it and the session's header carries it
 * (`HeaderEntry.commands`, passed as it is): its name without the slash, what it does, and a hint for its input. */
export type SlashCommand = { name: string; description: string; input_hint?: string };

/** The `/` menu, its filtering and its caret measurement: one late chunk, loaded as the field is first focused. */
const CommandMenu = lazy(() =>
  late(import("./composer-menu.tsx").then((m) => ({ default: m.CommandMenu }))),
);

/** A paper added to the question, as a small raised pill on the field's well: the paper book page glyph, its short
 * title and page, and a press to take it off again. */
export type QuestionPaper = Pick<Paper, "id" | "title" | "pb">;

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

/** What the room may do to the field from outside. The field is uncontrolled: React never writes its value, so the
 * engine's undo stack stays whole through an input method's edits, and every change made here goes through the
 * engine's own editing (`execCommand`), so Cmd/Ctrl+Z takes it back. */
export type ComposerHandle = {
  focus: () => void;
  /** What the field holds now. */
  text: () => string;
  /** Puts the field's focus in it and replaces all it holds with `text`, as one step undo takes back. */
  setText: (text: string) => void;
  /** Empties the field as one undoable step: with the field focused, or with focus on nothing or on a button of the
   * field's own row (Ask, just pressed), which it takes back to the field. With focus anywhere else on the page it is
   * emptied in place, so the lawyer's focus never moves, and that one step can't be undone. Given `only`, it clears
   * only while the field holds exactly that. While an input method composes it waits for the composition's end and
   * clears then if the field still holds what it held. */
  clear: (only?: string) => void;
  /** Puts `text` into the field only while it is empty, as one undoable step, and never moves the lawyer's focus:
   * it takes the focus only where clear may (mayTakeFocus), and anywhere else the words go in place. While an input
   * method composes it waits for the composition's end, and fills then only if the field is still empty. */
  fill: (text: string) => void;
};

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
  /** The words the field starts with, such as the matter's draft. Read once, as the field mounts: key the composer to
   * start it again with other words. */
  initialText?: string;
  /** Hears what the field holds after each change, an input method's composing included, read from the field. */
  onInput?: (text: string) => void;
  /** The handle: focus, the field's words, and setting or clearing them so undo survives. */
  ref?: Ref<ComposerHandle>;
  papers?: readonly QuestionPaper[];
  onRemovePaper?: (id: string) => void;
  choice?: PlanChoice<V>;
  /** Keys the field passes on first, before its own Enter. True when one was taken, and its default is prevented.
   * While an input method composes it hears nothing: every key is the input method's then. */
  onKeyCommand?: (e: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  /** The field's id, unique on the page. */
  id?: string;
  /** The one act left before the field can take a question, such as signing in to a plan: it stands in the field's
   * row in place of the words, Add papers and Ask, on the same well. */
  gate?: ReactNode;
  /** Words in the room's line's place, such as the fine print before signing in; false for no line at all. */
  line?: ReactNode | false;
  /** No matter open: the field, Add papers and Ask take nothing. */
  disabled?: boolean;
  /** data-testid on the field, Ask, Stop, the working line's row, and the `/` menu and each of its rows. */
  testIds?: {
    field?: string;
    ask?: string;
    stop?: string;
    working?: string;
    menu?: string;
    menuItem?: string;
  };
  /** The agent's commands, for the `/` menu: `/` at the field's start or after a space opens it at the caret, and a
   * list replaced while it is open is read again. Without them `/` is only a character. */
  commands?: readonly SlashCommand[];
  /** Questions drawn from the matter's papers, offered over an empty field: a press puts one in the field. */
  prompts?: { label: string; items: readonly { text: string }[] };
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
  /** Ask, pressed, or Enter with nothing composing, with what the field holds. Enter is always the composer's: it
   * never breaks the line (Shift+Enter does), even when busy or empty; whether anything goes is the caller's call. */
  onAsk?: (text: string) => void;
  /** Add papers, pressed. Add papers shows only when given. */
  onAddPapers?: () => void;
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
  initialText = "",
  onInput,
  ref,
  papers = [],
  onRemovePaper,
  choice,
  onKeyCommand,
  id = "ask",
  gate,
  line,
  disabled = false,
  testIds,
  commands,
  prompts,
  notice,
  queue,
  acts,
  placeholder,
  onAsk,
  onAddPapers,
}: ComposerProps<V>) {
  const w = useThreadWords();
  const room = useRoom();
  // the one thing the composer draws from the words: whether there are any, for Ask and the questions to ask. It
  // changes only when the field fills or empties, so a keystroke renders nothing
  // pinned at mount: React writes a textarea's defaultValue (its text child) on every commit, and the caller's draft
  // changes as the lawyer types, so passing the prop through would rewrite the field's child mid-composition
  const [initial] = useState(initialText);
  const [empty, setEmpty] = useState(() => initial.trim() === "");
  const area = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false);
  // a clear asked for while an input method composed: what the field held then, cleared at the composition's end
  // only if the field still holds it
  const clearAfter = useRef<string | null>(null);
  // a fill asked for while an input method composed: put in at the composition's end, if the field is empty then
  const fillAfter = useRef<string | null>(null);
  const fitRef = useRef<() => void>(() => {});
  // the menu's chunk is asked for as the field is first focused, with what the field held then, so a `/` typed
  // before it lands is read as it lands
  const [armed, setArmed] = useState<{ field: HTMLTextAreaElement; held: string } | null>(null);
  const [shown, setShown] = useState<MenuShown>(null);
  const hasCommands = commands !== undefined;
  // commands that arrive while the field already has the focus arm it too
  useEffect(() => {
    const a = area.current;
    if (hasCommands && a !== null && a.ownerDocument.activeElement === a)
      setArmed((was) => was ?? { field: a, held: a.value });
  }, [hasCommands]);
  const gated = gate !== undefined;
  // the room's field is the one the lawyer can see: hidden under a gate, it is no field for the room's keys or focus
  const fieldRef = useCallback(
    (node: HTMLTextAreaElement | null) => {
      area.current = node;
      if (node === null || gated) return;
      return room.nodes.slot("field")(node);
    },
    [room, gated],
  );
  const entry = useRef<HTMLDivElement>(null);
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
      const cap = one * 6;
      if (a.scrollHeight > one + 4) a.style.height = `${Math.min(a.scrollHeight, cap)}px`;
      // only words past the six lines scroll; short of them the field draws no scroll arrows. A Devanagari line can
      // measure a few px over the one line, inside the slack above, and WebView2 drew arrows on an empty field
      const scrolls = a.scrollHeight > cap;
      a.style.overflowY = scrolls ? "" : "hidden";
      if (!scrolls) a.scrollTop = 0;
    };
    fitRef.current = fit;
    fit();
    // the fit can change the field's height and so the form's, which the status line's observer, watching the form,
    // has already been told of this frame: the browser keeps that change for the next frame and reports
    // "ResizeObserver loop completed with undelivered notifications" once. It is no loop: the next delivery finds
    // the width unchanged and fits nothing. (Chromium, a composer at 390 px whose width alone changes.)
    // The WebKit probe must allow this one message ("ResizeObserver loop completed with undelivered notifications"),
    // decided in review: it fires once per width change and settles.
    let width = a.clientWidth;
    const ro = new ResizeObserver(() => {
      if (a.clientWidth === width) return;
      width = a.clientWidth;
      fit();
    });
    ro.observe(a);
    return () => ro.disconnect();
  }, [lang, textSize]);
  // after every change to the words, typed, composed, undone or set through the handle: fit the field to them, say
  // whether it holds any, and tell the room
  const changed = () => {
    const a = area.current;
    if (a === null) return;
    fitRef.current();
    setEmpty(a.value.trim() === "");
    onInput?.(a.value);
  };
  const put = (next: string) => {
    const a = area.current;
    if (a === null) return;
    if (!replaceAll(a, next, true)) changed();
    a.focus();
  };
  const clear = (only?: string, inPlace = false) => {
    const a = area.current;
    if (a === null || (only !== undefined && a.value !== only)) return;
    if (composing.current) {
      // select-all and delete inside a composition would break it: wait for its end
      clearAfter.current = a.value;
      return;
    }
    if (inPlace) a.value = "";
    if (inPlace || !replaceAll(a, "", mayTakeFocus(a))) changed();
  };
  const fill = (text: string, inPlace = false) => {
    const a = area.current;
    if (a === null) return;
    if (composing.current) {
      // inserting inside a composition would break it: wait for its end
      fillAfter.current = text;
      return;
    }
    if (a.value !== "") return;
    if (inPlace) a.value = text;
    if (inPlace || !replaceAll(a, text, mayTakeFocus(a))) changed();
  };
  // the held clear and fill, once the composition is over; in place when the field is losing focus, which it never
  // takes back
  const settle = (inPlace: boolean) => {
    const held = clearAfter.current;
    clearAfter.current = null;
    if (held !== null) clear(held, inPlace);
    const filling = fillAfter.current;
    fillAfter.current = null;
    if (filling !== null) fill(filling, inPlace);
  };
  // made anew each render, as changed reads the caller's onInput: the composer renders rarely, never per keystroke
  useImperativeHandle(ref, () => ({
    focus: () => area.current?.focus(),
    text: () => area.current?.value ?? "",
    setText: put,
    clear: (only) => clear(only),
    fill: (text) => fill(text),
  }));
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
        onAsk?.(area.current?.value ?? "");
      }}
    >
      {queue}
      {prompts !== undefined && prompts.items.length > 0 && empty && gate === undefined && (
        <ul className="room-prompts" aria-label={prompts.label}>
          {prompts.items.map((p) => (
            <li key={p.text}>
              <button type="button" className="room-prompt" onClick={() => put(p.text)}>
                <Say>{p.text}</Say>
              </button>
            </li>
          ))}
        </ul>
      )}
      {acts}
      {/* a status line with nothing to say isn't drawn: no time yet, no plan, no choice and no hint */}
      {(took !== null || plan !== "" || choice !== undefined || hint !== null) && (
        <StatusLine took={took} plan={plan} hint={hint} choice={choice} hidden={working !== null} />
      )}
      {/* while Clauseo works the working line is the well's top row, and the field its foot: one set-in region */}
      <div className="room-entry" ref={entry} data-working={working !== null ? "" : undefined}>
        {/* what assistive technology hears of the working line: one status region, mounted as long as the composer
            is and empty while nothing stands, so a line that comes, changes or goes is said once, in place */}
        <span className="sr-only" role="status">
          {working !== null && <WorkingSaid state={working} />}
        </span>
        {working !== null && <WorkingLine state={working} testId={testIds?.working} />}
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
          {gate}
          {gate === undefined && (
            <>
              {papers.length > 0 && <PaperChips papers={papers} onRemove={onRemovePaper} />}
              <label className="sr-only" htmlFor={fieldId}>
                {placeholder ?? w.t("Ask about this matter")}
              </label>
            </>
          )}
          {/* under a gate the field stays, hidden, so the words typed and the engine's undo for them are there
              again when the gate goes */}
          <textarea
            hidden={gate !== undefined}
            id={fieldId}
            ref={fieldRef}
            rows={1}
            defaultValue={initial}
            disabled={disabled}
            data-testid={testIds?.field}
            placeholder={placeholder ?? w.t("Ask about this matter")}
            // the `/` menu: a textarea takes no combobox role and no aria-expanded (ARIA in HTML; axe's
            // aria-allowed-role and aria-allowed-attr), so it says it has a list and, while open, which row is lit
            aria-autocomplete={commands !== undefined ? "list" : undefined}
            aria-haspopup={commands !== undefined ? "listbox" : undefined}
            aria-controls={shown?.list}
            aria-activedescendant={shown?.active}
            data-menu={shown !== null ? "" : undefined}
            onFocus={(e) => {
              if (armed === null && commands !== undefined)
                setArmed({ field: e.currentTarget, held: e.currentTarget.value });
            }}
            onInput={changed}
            onCompositionStart={() => {
              composing.current = true;
            }}
            onCompositionEnd={() => {
              composing.current = false;
              // a clear held for the composition runs once the engine has settled the value, as the spike's
              // triggers do
              if (clearAfter.current !== null || fillAfter.current !== null)
                setTimeout(() => settle(false), 0);
            }}
            onBlur={() => {
              // a composition the engine ended without its compositionend must not hold Enter, or a clear, for
              // good
              composing.current = false;
              settle(true);
            }}
            onKeyDown={(e) => {
              const key = {
                key: e.key,
                shiftKey: e.shiftKey,
                isComposing: composing.current || e.nativeEvent.isComposing,
                keyCode: e.keyCode,
              };
              // while an input method composes, every key is its own: an Enter commits its candidate. A key the open
              // menu took (its listener on the field runs first) is the menu's
              if (key.isComposing || key.keyCode === 229 || e.nativeEvent.defaultPrevented) return;
              if (onKeyCommand?.(e) === true) {
                e.preventDefault();
                return;
              }
              if (!sendsOnEnter(key)) return;
              e.preventDefault();
              onAsk?.(e.currentTarget.value);
            }}
          />
          {armed !== null &&
            commands !== undefined &&
            commands.length > 0 &&
            !gated &&
            !disabled && (
              <Suspense fallback={null}>
                <CommandMenu
                  field={armed.field}
                  commands={commands}
                  composing={composing}
                  held={armed.held}
                  onShown={setShown}
                  testIds={{ menu: testIds?.menu, item: testIds?.menuItem }}
                />
              </Suspense>
            )}
          {gate === undefined && (
            <>
              {onAddPapers !== undefined && (
                <Button
                  kind="quiet"
                  shape="icon"
                  className="room-field-add"
                  aria-label={w.t("Add papers")}
                  disabled={disabled}
                  onClick={onAddPapers}
                >
                  <IconAddPapers size={16} />
                </Button>
              )}
              {working !== null && <WorkingTime state={working} />}
              {busy ? (
                <Button
                  kind="tone"
                  className="room-field-ask"
                  aria-label={w.th("Stop the answer")}
                  onClick={onStop}
                  data-testid={testIds?.stop}
                >
                  <span className="deva-nudge">{w.t("Stop")}</span>
                </Button>
              ) : (
                <Button
                  kind="primary"
                  type="submit"
                  className="room-field-ask"
                  disabled={disabled || empty}
                  data-testid={testIds?.ask}
                >
                  {w.t("Ask")}
                </Button>
              )}
            </>
          )}
        </div>
      </div>
      {notice}
      {line !== false && (
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
      )}
    </form>
  );
}
