// The agent's question as a form, inline in the thread, on the permission prompt's well: a court-yellow dot and "A
// question for you", the agent's words, its fields (elicitation-schema.ts), then Send the answer, Decline and Stop the
// answer. Send waits until every field holds and something is filled in: an answer with nothing in it is Decline.
// One answer at a time; a refusal says why under the answers and keeps the form as the lawyer left it. Enter in a
// single-line field, a radio or a checkbox sends; in a textarea it's a new line, and Cmd or Ctrl with Enter sends.
// The form takes focus as it appears only from the page or the thread, never from the composer or anywhere else.
// Everything the agent wrote is set in <bdi> or dir="auto".
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { IconTick } from "@clauseo/components/icons/icons.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { fieldError, textControl } from "@clauseo/components/ui/recipes-controls.ts";
import { toHostError } from "../transport/host-transport.ts";
import type { ElicitationId, ElicitationResponse, RawPayload } from "../transport/protocol.ts";
import {
  canSend,
  content,
  anyFilled,
  initialDraft,
  parseForm,
  problems,
  type Choice,
  type Draft,
  type Field,
  type FieldProblem,
  type FieldValue,
  type TextField,
  ELICITATION_TEST_IDS,
} from "./elicitation-schema.ts";
import { useAgentPlan } from "./thread/agent-plan.ts";
import { useThreadActions } from "./thread/session.ts";

const T = ELICITATION_TEST_IDS;

/** A native radio or checkbox, drawn as the design system's (recipes-controls.ts's radio and checkBox): set into the
 * plane with its lip, primary once chosen. Native, so a shared name gives the arrows within a group for free. */
const NATIVE = [
  "peer col-start-1 row-start-1 m-0 appearance-none size-check motion-control",
  "bg-inset set-in-lip is-hover:bg-well-strong is-active:bg-inset",
  "checked:bg-primary checked:shadow-icon-primary is-hover:checked:bg-primary-hover",
  "is-disabled:bg-well-strong is-disabled:shadow-none",
].join(" ");
const MARK = "relative pointer-events-none col-start-1 row-start-1 hidden peer-checked:block";

/** A field's ids, and whether its problem shows. */
type Ids = { base: string; problem: string | null };

type Props = {
  elicitation: ElicitationId;
  message: string;
  schema: RawPayload;
};

/** A pending form elicitation. */
export function ElicitationForm({ elicitation, message, schema }: Props) {
  const w = useThreadWords();
  const lang = w.lang;
  const plan = useAgentPlan();
  const room = useRoom();
  const { answerElicitation, stop } = useThreadActions();
  const uid = useId();
  const form = useMemo(() => parseForm(schema), [schema]);
  const [draft, setDraft] = useState<Draft>(() => (form === null ? {} : initialDraft(form)));
  const [touched, setTouched] = useState<ReadonlySet<string>>(new Set());
  const [attempted, setAttempted] = useState(false);
  const [asked, setAsked] = useState<"accept" | "decline" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const flying = useRef(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const fieldsRef = useRef<HTMLDivElement>(null);

  const found = useMemo(
    () => (form === null ? new Map<string, FieldProblem>() : problems(form, draft)),
    [form, draft],
  );
  const filled = form !== null && anyFilled(form, draft);
  const ready = form !== null && canSend(form, draft);
  const busy = asked !== null;

  // the form takes focus as it appears only from nowhere (the page) or the thread, never from the composer
  useEffect(() => {
    const now = document.activeElement;
    const thread = room.nodes.get("thread");
    const free =
      now === null ||
      now === document.body ||
      rootRef.current?.contains(now) === true ||
      // the thread, but never another question's form: the first question on the page keeps it
      (thread?.contains(now) === true && now.closest(`[data-testid="${T.form}"]`) === null);
    if (!free) return;
    const first =
      fieldsRef.current?.querySelector<HTMLElement>("input:checked") ??
      fieldsRef.current?.querySelector<HTMLElement>("input, textarea") ??
      null;
    first?.focus({ preventScroll: true });
    // as it appears only: the room's scope stays the same for the row's life
  }, [room]);

  const respond = useCallback(
    (kind: "accept" | "decline", response: ElicitationResponse) => {
      if (flying.current) return;
      flying.current = true;
      setError(null);
      setAsked(kind);
      answerElicitation(elicitation, response).then(
        () => {
          // answered: the entry folds once the host says so; focus inside the form goes back to the composer's field
          const now = document.activeElement;
          if (now === null || now === document.body || rootRef.current?.contains(now) === true)
            room.nodes.get("field")?.focus();
        },
        (e: unknown) => {
          flying.current = false;
          setAsked(null);
          setError(errorWords(lang, toHostError(e).code, plan));
        },
      );
    },
    [answerElicitation, elicitation, lang, plan, room],
  );

  const send = useCallback(() => {
    if (form === null || busy) return;
    if (!ready) return setAttempted(true);
    respond("accept", { accept: { values: content(form, draft) } });
  }, [form, busy, ready, respond, draft]);
  const decline = useCallback(() => {
    if (!busy) respond("decline", "decline");
  }, [busy, respond]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "Enter" || e.nativeEvent.isComposing || e.defaultPrevented) return;
    const t = e.target;
    if (t instanceof HTMLTextAreaElement) {
      if (!e.metaKey && !e.ctrlKey) return;
    } else if (!(t instanceof HTMLInputElement) || e.shiftKey) return;
    e.preventDefault();
    send();
  };

  const set = useCallback((key: string, v: FieldValue) => {
    setDraft((d) => ({ ...d, [key]: v }));
  }, []);
  const touch = useCallback((key: string) => {
    setTouched((s) => (s.has(key) ? s : new Set(s).add(key)));
  }, []);
  /** A field's ids: its problem line's id only while it shows (a required one once Send was pressed, another once
   * the field was left). */
  const idsOf = (key: string, slot: string): Ids => {
    const p = found.get(key);
    const shows = p !== undefined && (attempted || (p === "invalid" && touched.has(key)));
    return { base: `${uid}-f${slot}`, problem: shows ? `${uid}-f${slot}-problem` : null };
  };
  const problemLine = (key: string, ids: Ids) =>
    ids.problem === null ? null : (
      <p id={ids.problem} className={`${fieldError} mt-s1`} data-testid={T.problem(key)}>
        <Say>
          {found.get(key) === "required"
            ? w.th("Answer this question.")
            : w.th("This answer doesn't fit the question.")}
        </Say>
      </p>
    );
  const required = <Say>{w.th("Required")}</Say>;
  const ctx: FieldCtx = { draft, set, touch, idsOf, problemLine, required, other: w.th("Other") };

  const askId = `${uid}-ask`;
  const msgId = `${uid}-msg`;
  const hintId = `${uid}-hint`;
  const hint = form !== null && !filled && !busy;
  return (
    <div
      ref={rootRef}
      className="perm"
      role="group"
      aria-labelledby={askId}
      aria-describedby={msgId}
      aria-busy={busy || undefined}
      data-testid={T.form}
      data-supported={form === null ? "false" : "true"}
      onKeyDown={onKeyDown}
    >
      <p className="perm-ask">
        <span className="flex flex-none w-s4 justify-center" aria-hidden="true">
          <span className="size-dot-hint mt-s2 rounded-full bg-yellow shadow-dot" />
        </span>
        <span id={askId}>
          <Say>{frame(lang, "A question for you")}</Say>
        </span>
      </p>
      <p
        id={msgId}
        className="mt-s0-5 pl-(--perm-indent) text-ui text-primary whitespace-pre-wrap"
        dir="auto"
      >
        <bdi>{message}</bdi>
      </p>
      {form !== null && (
        <div ref={fieldsRef} className="mt-s3 pl-(--perm-indent) grid gap-s4">
          {form.fields.map((f, i) => (
            <FieldView key={f.key} field={f} index={i} ctx={ctx} />
          ))}
        </div>
      )}
      <div className="perm-actions">
        {form !== null && (
          <Button
            kind="tone"
            size="small"
            data-testid={T.send}
            aria-disabled={!ready || busy ? true : undefined}
            aria-describedby={hint ? hintId : undefined}
            aria-keyshortcuts="Enter"
            onClick={send}
          >
            <span className="deva-nudge">{w.th("Send the answer")}</span>
          </Button>
        )}
        <Button
          kind="quiet"
          size="small"
          data-testid={T.decline}
          aria-disabled={busy ? true : undefined}
          onClick={decline}
        >
          <span className="deva-nudge">{w.th("Decline")}</span>
        </Button>
        <Button kind="quiet" size="small" data-testid={T.stop} onClick={stop}>
          <span className="deva-nudge">{w.th("Stop the answer")}</span>
        </Button>
      </div>
      {hint && (
        <p id={hintId} className="perm-kept mt-s2" data-testid={T.hint}>
          <Say>{w.th("Choose an answer to send, or Decline to go on without one.")}</Say>
        </p>
      )}
      {error ? (
        <p
          className="perm-error text-small text-quiet mt-(--s2) pl-(--perm-indent)"
          role="status"
          data-testid={T.error}
        >
          <Say>{error}</Say>
        </p>
      ) : null}
    </div>
  );
}

type FieldCtx = {
  draft: Draft;
  set: (key: string, v: FieldValue) => void;
  touch: (key: string) => void;
  idsOf: (key: string, slot: string) => Ids;
  problemLine: (key: string, ids: Ids) => ReactNode;
  required: ReactNode;
  other: string;
};

const ids = (...list: (string | null | false | undefined)[]) =>
  list.filter(Boolean).join(" ") || undefined;

/** A field's name: its title, else its key. */
const nameOf = (f: Field) => f.title ?? f.key;

function Marker({ children }: { children: ReactNode }) {
  return <span className="ms-s2 text-small text-muted">{children}</span>;
}

function FieldView({ field: f, index, ctx }: { field: Field; index: number; ctx: FieldCtx }) {
  const own = ctx.idsOf(f.key, String(index));
  const descId = f.description === null ? null : `${own.base}-desc`;
  const desc =
    f.description === null ? null : (
      <p id={descId ?? undefined} className="mt-s0-5 text-small text-muted" dir="auto">
        <bdi>{f.description}</bdi>
      </p>
    );
  if (f.kind === "choice" || f.kind === "multi") {
    const v = ctx.draft[f.key];
    const multi = f.kind === "multi";
    return (
      <fieldset
        className="m-0 p-0 border-0 min-w-0"
        role={multi ? undefined : "radiogroup"}
        aria-describedby={ids(descId, own.problem)}
        aria-required={!multi && f.required ? true : undefined}
        aria-invalid={own.problem === null ? undefined : true}
        data-testid={T.field(f.key)}
      >
        <legend className="p-0 text-ui font-ui text-primary">
          <bdi>{nameOf(f)}</bdi>
          {f.required && <Marker>{ctx.required}</Marker>}
        </legend>
        {desc}
        <div className="mt-s1 grid">
          {f.choices.map((c, i) => (
            <Option
              key={i}
              choice={c}
              multi={multi}
              name={own.base}
              testId={T.choice(f.key, i)}
              checked={multi ? Array.isArray(v) && v.includes(i) : v === i}
              required={!multi && f.required}
              onChange={(on) => {
                if (!multi) return ctx.set(f.key, i);
                const was = Array.isArray(v) ? v : [];
                ctx.set(f.key, on ? [...was, i].sort((a, b) => a - b) : was.filter((x) => x !== i));
                ctx.touch(f.key);
              }}
            />
          ))}
        </div>
        {f.other !== null && <Other field={f.other} index={index} ctx={ctx} />}
        {ctx.problemLine(f.key, own)}
      </fieldset>
    );
  }
  if (f.kind === "boolean")
    return (
      <div data-testid={T.field(f.key)}>
        <label className="flex items-start gap-s3">
          <span className="flex flex-none items-center h-ui-line">
            <span className="inline-grid place-items-center">
              <input
                type="checkbox"
                className={`${NATIVE} rounded-label`}
                checked={ctx.draft[f.key] === true}
                onChange={(e) => ctx.set(f.key, e.currentTarget.checked)}
                aria-describedby={ids(descId)}
                data-testid={T.input(f.key)}
              />
              <IconTick size={14} className={`${MARK} text-on-primary`} />
            </span>
          </span>
          <span className="min-w-0 text-ui font-ui text-primary">
            <bdi>{nameOf(f)}</bdi>
          </span>
        </label>
        {desc}
      </div>
    );
  return <Typed field={f} index={index} ctx={ctx} desc={desc} descId={descId} />;
}

/** A text or number field, standing on its own: its label, the control, its line and its problem. */
function Typed({
  field: f,
  index,
  ctx,
  desc,
  descId,
}: {
  field: Extract<Field, { kind: "text" | "number" }>;
  index: number;
  ctx: FieldCtx;
  desc: ReactNode;
  descId: string | null;
}) {
  const own = ctx.idsOf(f.key, String(index));
  const v = ctx.draft[f.key];
  const value = typeof v === "string" ? v : "";
  const common = {
    id: own.base,
    value,
    dir: "auto" as const,
    required: f.required,
    "aria-required": f.required || undefined,
    "aria-invalid": own.problem === null ? undefined : true,
    "aria-describedby": ids(descId, own.problem),
    "data-testid": T.input(f.key),
    onBlur: () => ctx.touch(f.key),
  };
  const control =
    f.kind === "number" ? (
      <input
        {...common}
        type="number"
        inputMode={f.integer ? "numeric" : "decimal"}
        step={f.integer ? 1 : "any"}
        min={f.minimum ?? undefined}
        max={f.maximum ?? undefined}
        className={`${textControl} h-control`}
        onChange={(e) => ctx.set(f.key, e.currentTarget.value)}
      />
    ) : (
      <TextControl field={f} common={common} ctx={ctx} />
    );
  return (
    <div className="grid gap-s1-5 min-w-0" data-testid={T.field(f.key)}>
      <label htmlFor={own.base} className="text-ui font-ui text-primary">
        <bdi>{nameOf(f)}</bdi>
        {f.required && <Marker>{ctx.required}</Marker>}
      </label>
      {control}
      {desc}
      {ctx.problemLine(f.key, own)}
    </div>
  );
}

type Common = {
  id: string;
  value: string;
  dir: "auto";
  required: boolean;
  "aria-required": boolean | undefined;
  "aria-invalid": boolean | undefined;
  "aria-describedby": string | undefined;
  "data-testid": string;
  onBlur: () => void;
};

function TextControl({
  field: f,
  common,
  ctx,
}: {
  field: TextField;
  common: Common;
  ctx: FieldCtx;
}) {
  const limits = {
    minLength: f.minLength ?? undefined,
    maxLength: f.maxLength ?? undefined,
  };
  return f.multiline ? (
    <textarea
      {...common}
      {...limits}
      rows={2}
      className={`${textControl} py-s2 resize-y`}
      onChange={(e) => ctx.set(f.key, e.currentTarget.value)}
    />
  ) : (
    <input
      {...common}
      {...limits}
      type={f.input}
      className={`${textControl} h-control`}
      onChange={(e) => ctx.set(f.key, e.currentTarget.value)}
    />
  );
}

/** A question's Other: our word for its label, the agent's description as its hint, a short textarea beneath the
 * options, inside the question's group. Independent of the pick. */
function Other({ field: f, index, ctx }: { field: TextField; index: number; ctx: FieldCtx }) {
  const own = ctx.idsOf(f.key, `${index}o`);
  const id = own.base;
  const hintId = f.description === null ? null : `${id}-hint`;
  const v = ctx.draft[f.key];
  return (
    <div className="mt-s2 grid gap-s1">
      <label htmlFor={id} className="text-small font-ui text-primary">
        <Say>{ctx.other}</Say>
      </label>
      <textarea
        id={id}
        rows={2}
        dir="auto"
        value={typeof v === "string" ? v : ""}
        maxLength={f.maxLength ?? undefined}
        aria-describedby={ids(hintId, own.problem)}
        aria-invalid={own.problem === null ? undefined : true}
        data-testid={T.input(f.key)}
        className={`${textControl} py-s2 resize-y`}
        onChange={(e) => ctx.set(f.key, e.currentTarget.value)}
        onBlur={() => ctx.touch(f.key)}
      />
      {hintId !== null && (
        <p id={hintId} className="text-small text-muted" dir="auto">
          <bdi>{f.description}</bdi>
        </p>
      )}
      {ctx.problemLine(f.key, own)}
    </div>
  );
}

/** One option of a question: a native radio or checkbox, its title and its line. */
function Option({
  choice,
  multi,
  name,
  testId,
  checked,
  required,
  onChange,
}: {
  choice: Choice;
  multi: boolean;
  name: string;
  testId: string;
  checked: boolean;
  required: boolean;
  onChange: (on: boolean) => void;
}) {
  const lineId = `${name}-${testId}-line`;
  return (
    <label className="flex items-start gap-s3 py-s1-5">
      <span className="flex flex-none items-center h-ui-line">
        <span className="inline-grid place-items-center">
          <input
            type={multi ? "checkbox" : "radio"}
            name={name}
            className={`${NATIVE} ${multi ? "rounded-label" : "rounded-full"}`}
            checked={checked}
            required={required || undefined}
            aria-describedby={choice.description === null ? undefined : lineId}
            data-testid={testId}
            onChange={(e) => onChange(e.currentTarget.checked)}
          />
          {multi ? (
            <IconTick size={14} className={`${MARK} text-on-primary`} />
          ) : (
            <span className={`${MARK} size-check-dot rounded-full bg-on-primary`} />
          )}
        </span>
      </span>
      <span className="min-w-0">
        <span className="block text-ui font-ui text-primary" dir="auto">
          <bdi>{choice.title}</bdi>
        </span>
        {choice.description !== null && (
          <span id={lineId} className="block mt-s0-5 text-small text-muted" dir="auto">
            <bdi>{choice.description}</bdi>
          </span>
        )}
      </span>
    </label>
  );
}
