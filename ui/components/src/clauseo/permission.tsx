// Asking before going on. When Clauseo wants to do something the lawyer must allow, saving a new version of a paper
// for one, it asks inline, in the thread, never in a modal: what it wants to do, that every change is kept, and the
// answers the agent offers, in ACP's kind order. Enter allows once, Cmd or Ctrl with Enter allows for this matter,
// Escape declines; the keys act from the empty composer, from the page or from inside the prompt, without taking the
// composer's focus. Once answered, the prompt folds to one line that says what was decided.
import { useCallback, useEffect, useId, useLayoutEffect, useRef } from "react";
import { Button } from "../ui/button.tsx";
import { Kbd } from "../ui/kbd.tsx";
import { useDisplay } from "../lib/display.ts";
import { IconAccept, IconNewMatter, IconPaperBookPage } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { useVersionWords } from "../lib/words-versions.ts";
import {
  ANSWER_ORDER,
  keyReachesPrompt,
  permissionKey,
  type KeyWhere,
  type PermissionAnswer,
  type PermissionState,
} from "./permission-keys.ts";
import { fromComposer } from "./composer-keys.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";

const OFFERED: readonly PermissionAnswer[] = ["once", "always", "deny"];
const LABEL = {
  once: "Allow once",
  always: "Always for this matter",
  deny: "Don't allow",
  never: "Don't allow for this matter",
} as const;

export function PermissionPrompt({
  paper,
  state,
  onAnswer,
  keys = false,
  offered = OFFERED,
  question,
  title,
  kept = true,
  done,
  confirming = false,
  busy = false,
  error,
  testIds,
  dataState,
  dataAnswer,
}: {
  /** The paper's name, as the wall shows it: "Notice to the bank"; null when the agent names none. */
  paper: string | null;
  state: PermissionState;
  onAnswer: (a: PermissionAnswer) => void;
  /** Whether this prompt takes the keys: one prompt on the page at a time. */
  keys?: boolean;
  /** The answers the agent offers; the buttons stand in ACP's kind order whatever order these come in. */
  offered?: readonly PermissionAnswer[];
  /** The ask line, already in words, in place of the one about saving the paper. */
  question?: string;
  /** The agent's own words for what it wants to do, beneath the question. */
  title?: string | null;
  /** Whether "Every change is kept." shows. */
  kept?: boolean;
  /** Whether the tool call finished; left out, the folded line names the paper as before. */
  done?: boolean | undefined;
  /** The computer's own dialog is up: the answers wait and the prompt says so. */
  confirming?: boolean;
  /** An answer is on its way: the answers wait. */
  busy?: boolean;
  /** A line under the answers, already in words. */
  error?: string | null;
  testIds?: { root: string; answer: (a: PermissionAnswer) => string };
  dataState?: string;
  dataAnswer?: string;
}) {
  const w = useThreadWords();
  const v = useVersionWords();
  const id = useId();
  const { platform } = useDisplay();
  const mac = platform === "mac";
  const room = useRoom();
  const rootRef = useRef<HTMLDivElement>(null);
  // the latest answer handler and offer, so a caller's new closure or array doesn't re-register the listener
  const latest = useRef({ onAnswer, offered });
  useEffect(() => {
    latest.current = { onAnswer, offered };
  });

  // Focus inside the prompt goes back to the room's field after an answer, and when the prompt goes inert or folds.
  // Whether it was inside is tracked by focus and blur, since inert blurs the button and the fold removes it: a removed
  // button's blur never reaches React, so the note survives the fold.
  const focusInside = useRef(false);
  const away = state !== "pending" || busy || confirming;
  useLayoutEffect(() => {
    if (!away || !focusInside.current) return;
    const now = document.activeElement;
    const lost = now === null || now === document.body || rootRef.current?.contains(now) === true;
    focusInside.current = false;
    if (lost) room.nodes.get("field")?.focus();
  }, [away, room]);
  const answer = useCallback(
    (a: PermissionAnswer) => {
      const inside = rootRef.current?.contains(document.activeElement) === true;
      latest.current.onAnswer(a);
      if (inside) room.nodes.get("field")?.focus();
    },
    [room],
  );

  const listening = keys && state === "pending" && !busy && !confirming;
  useEffect(() => {
    if (!listening) return;
    // capture: the composer takes Enter in its own handler and prevents it, so a bubbling listener would never see it
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      const a = permissionKey(e);
      if (a === null || !latest.current.offered.includes(a)) return;
      const target = e.target;
      let where: KeyWhere = "other";
      let fieldEmpty = false;
      const roomField = room.nodes.get("field");
      if (target instanceof Node && rootRef.current?.contains(target)) {
        // a focused answer keeps its own plain Enter: its click answers
        if (a === "once" && target instanceof HTMLButtonElement) return;
        where = "prompt";
      } else if (fromComposer(room, target)) {
        if (target instanceof HTMLTextAreaElement) {
          where = "field";
          fieldEmpty = target.value.trim() === "";
        } else {
          // nothing focused: only while the room's own field is empty, and never with no field
          where = "page";
          fieldEmpty = roomField instanceof HTMLTextAreaElement && roomField.value.trim() === "";
        }
      }
      if (!keyReachesPrompt(where, fieldEmpty)) return;
      e.preventDefault();
      e.stopPropagation();
      answer(a);
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [listening, room, answer]);

  const rootData = {
    "data-testid": testIds?.root,
    "data-state": dataState ?? state,
    "data-answer": dataAnswer,
  };

  if (state !== "pending") {
    // what the lawyer decided, then what Clauseo did, in the past tense
    const said = folded(w.th, state, paper, done);
    const refused = state === "deny" || state === "never";
    return (
      <p
        className="perm-done"
        aria-live="off"
        data-glyph={state === "gone" ? "none" : undefined}
        {...rootData}
      >
        {/* one quiet grammar in concrete: the accept glyph where it was allowed, a plain cross where it wasn't. Green
            means checked or supports, so it stays out */}
        {state === "gone" ? null : refused ? (
          <IconNewMatter size={14} className="perm-done-icon rotate-45" />
        ) : (
          <IconAccept size={14} className="perm-done-icon" />
        )}
        <span>
          <Say>{said}</Say>
        </span>
      </p>
    );
  }

  const ask =
    question ??
    (paper === null
      ? w.th("Clauseo asks before going on.")
      : w.th("Clauseo wants to save a new version of {paper}.", { paper }));
  const shown = ANSWER_ORDER.filter((a) => offered.includes(a));
  const firstAllow = shown.find((a) => a === "once" || a === "always");
  const described = [title ? `${id}-title` : null, kept ? `${id}-kept` : null].filter(Boolean);
  const keysOn = keys && !busy && !confirming;
  const shortcut = (a: PermissionAnswer) =>
    a === "once" ? "Enter" : a === "always" ? (mac ? "Meta+Enter" : "Control+Enter") : "Escape";
  const cap = (a: PermissionAnswer) =>
    a === "once" ? "Enter" : a === "always" ? (mac ? "⌘ Enter" : "Ctrl Enter") : "Esc";
  return (
    <div
      ref={rootRef}
      className="perm"
      role="group"
      aria-labelledby={`${id}-ask`}
      aria-describedby={described.length > 0 ? described.join(" ") : undefined}
      data-confirming={confirming ? "" : undefined}
      onFocus={() => {
        focusInside.current = true;
      }}
      onBlur={(e) => {
        focusInside.current =
          e.relatedTarget instanceof Node && rootRef.current?.contains(e.relatedTarget) === true;
      }}
      {...rootData}
    >
      <p className="perm-ask">
        {paper !== null && <IconPaperBookPage size={16} className="perm-icon" />}
        <span id={`${id}-ask`}>
          <Say>{ask}</Say>
        </span>
      </p>
      {title ? (
        <p
          className="perm-title text-small text-quiet mt-(--s0-5) pl-(--perm-indent)"
          id={`${id}-title`}
        >
          <bdi>{title}</bdi>
        </p>
      ) : null}
      {kept ? (
        <p className="perm-kept" id={`${id}-kept`}>
          <Say>{w.t("Every change is kept.")}</Say>
        </p>
      ) : null}
      <div className="perm-actions" inert={busy || confirming ? true : undefined}>
        {shown.map((a) => {
          const withKey = keys && a !== "never";
          return (
            <Button
              key={a}
              kind={a === firstAllow ? "tone" : "quiet"}
              size="small"
              aria-keyshortcuts={withKey && keysOn ? shortcut(a) : undefined}
              data-testid={testIds?.answer(a)}
              onClick={() => answer(a)}
            >
              <span className="deva-nudge">{w.th(LABEL[a])}</span>
              {withKey && <Kbd className="perm-key">{cap(a)}</Kbd>}
            </Button>
          );
        })}
      </div>
      {/* Linux isn't a target yet, so it names no dialog */}
      {confirming && platform !== "linux" && (
        <p className="perm-confirming" role="status">
          <Say>
            {platform === "mac"
              ? v.v("macOS is asking you to confirm.")
              : v.v("Windows is asking you to confirm.")}
          </Say>
        </p>
      )}
      {error ? (
        <p className="perm-error text-small text-quiet mt-(--s2) pl-(--perm-indent)" role="status">
          <Say>{error}</Say>
        </p>
      ) : null}
    </div>
  );
}

/** The folded line: the answer, then, once the call is known, what came of it for the paper. */
function folded(
  th: ReturnType<typeof useThreadWords>["th"],
  state: Exclude<PermissionState, "pending">,
  paper: string | null,
  done: boolean | undefined,
): string {
  if (state === "gone") return th("No longer waiting for an answer.");
  if (state === "never") return th("Not allowed for this matter.");
  // done left out: the sentences from before the agent said whether the call finished
  const named = paper ?? "";
  if (done === undefined && paper !== null) {
    if (state === "once")
      return th("Allowed once. Clauseo saved a new version of {paper}.", { paper: named });
    if (state === "always")
      return th("Allowed for this matter. Clauseo saved a new version of {paper}.", {
        paper: named,
      });
    return th("Not allowed. Clauseo left {paper} as it was.", { paper: named });
  }
  if (state === "deny")
    return paper !== null && !done
      ? th("Not allowed. Clauseo left {paper} as it was.", { paper: named })
      : th("Not allowed.");
  const saved = done === true && paper !== null;
  if (state === "once")
    return saved
      ? th("Allowed once. Clauseo saved a new version of {paper}.", { paper: named })
      : th("Allowed once.");
  return saved
    ? th("Allowed for this matter. Clauseo saved a new version of {paper}.", { paper: named })
    : th("Allowed for this matter.");
}
