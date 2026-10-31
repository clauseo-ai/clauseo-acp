// Asking before a change. When Clauseo wants to save a new version of a paper, it asks inline,
// in the thread, never in a modal: what it wants to do, that every change is kept, and three answers. Enter allows
// once, Cmd or Ctrl with Enter allows for this matter, Escape declines; the keys act from the composer without
// taking its focus. Once answered, the prompt folds to one line that says what was decided.
import { useEffect, useId } from "react";
import { Button } from "../ui/button.tsx";
import { Kbd } from "../ui/kbd.tsx";
import { useDisplay } from "../lib/display.ts";
import { IconAccept, IconNewMatter, IconPaperBookPage } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { permissionKey, type PermissionAnswer, type PermissionState } from "./permission-keys.ts";
import { fromComposer } from "./composer-keys.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";

export function PermissionPrompt({
  paper,
  state,
  onAnswer,
  keys = false,
}: {
  /** The paper's name, as the wall shows it: "Notice to the bank". */
  paper: string;
  state: PermissionState;
  onAnswer: (a: PermissionAnswer) => void;
  /** Whether this prompt takes the keys: one prompt on the page at a time. */
  keys?: boolean;
}) {
  const w = useThreadWords();
  const id = useId();
  const mac = useDisplay().platform === "mac";
  const room = useRoom();

  useEffect(() => {
    if (!keys || state !== "pending") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing || !fromComposer(room, e.target)) return;
      const a = permissionKey(e);
      if (a === null) return;
      e.preventDefault();
      onAnswer(a);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [keys, state, onAnswer, room]);

  const ask = w.th("Clauseo wants to save a new version of {paper}.", { paper });
  if (state !== "pending") {
    // what the lawyer decided, then what Clauseo did, in the past tense
    const said =
      state === "once"
        ? w.th("Allowed once. Clauseo saved a new version of {paper}.", { paper })
        : state === "always"
          ? w.th("Allowed for this matter. Clauseo saved a new version of {paper}.", { paper })
          : w.th("Not allowed. Clauseo left {paper} as it was.", { paper });
    return (
      <p className="perm-done" data-state={state}>
        {/* one quiet grammar in concrete: the accept glyph where it was allowed, a plain cross where it wasn't. Green
            means checked or supports, so it stays out */}
        {state === "deny" ? (
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
  return (
    <div
      className="perm"
      role="group"
      aria-labelledby={`${id}-ask`}
      aria-describedby={`${id}-kept`}
    >
      <p className="perm-ask">
        <IconPaperBookPage size={16} className="perm-icon" />
        <span id={`${id}-ask`}>
          <Say>{ask}</Say>
        </span>
      </p>
      <p className="perm-kept" id={`${id}-kept`}>
        <Say>{w.t("Every change is kept.")}</Say>
      </p>
      <div className="perm-actions">
        <Button
          kind="tone"
          size="small"
          aria-keyshortcuts={keys ? "Enter" : undefined}
          onClick={() => onAnswer("once")}
        >
          <span className="deva-nudge">{w.th("Allow once")}</span>
          {keys && <Kbd className="perm-key">Enter</Kbd>}
        </Button>
        <Button
          kind="quiet"
          size="small"
          aria-keyshortcuts={keys ? (mac ? "Meta+Enter" : "Control+Enter") : undefined}
          onClick={() => onAnswer("always")}
        >
          <span className="deva-nudge">{w.th("Always for this matter")}</span>
          {keys && <Kbd className="perm-key">{mac ? "⌘ Enter" : "Ctrl Enter"}</Kbd>}
        </Button>
        <Button
          kind="quiet"
          size="small"
          aria-keyshortcuts={keys ? "Escape" : undefined}
          onClick={() => onAnswer("deny")}
        >
          <span className="deva-nudge">{w.th("Don't allow")}</span>
          {keys && <Kbd className="perm-key">Esc</Kbd>}
        </Button>
      </div>
    </div>
  );
}
