// The room's composer as the thread drives it: Ask, or Stop while a turn runs; Enter sends (the composer's own, through
// composer-keys.ts's sendsOnEnter: Shift+Enter breaks the line, and an input method's composing Enter is its own); the
// field keeps its words until the host accepts the question, and after a refusal. The working line says "Answering." while busy, and
// stands for an error by its code: a lost stream, with Try again, before a refused command.
import { useRef } from "react";
import { Composer, type ComposerHandle } from "@clauseo/components/clauseo/composer.tsx";
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { MatterId } from "../../transport/protocol.ts";
import type { Drafts } from "./session.ts";

export function WorkComposer({
  drafts,
  matter,
  disabled,
  busy,
  refused,
  failed,
  onAsk,
  onStop,
  onRetry,
}: {
  /** The room's drafts, by matter: the field starts with this matter's and keeps it there as it changes. */
  drafts: Drafts;
  matter: MatterId | null;
  /** No matter is open. */
  disabled: boolean;
  busy: boolean;
  /** The code of a refused command, until the next Ask. */
  refused: string | null;
  /** The code of the lost stream, until Try again brings it back. */
  failed: string | null;
  /** True once the question is accepted. */
  onAsk: (text: string) => Promise<boolean>;
  onStop: () => void;
  onRetry: () => void;
}) {
  const w = useWords();
  const field = useRef<ComposerHandle>(null);
  // the field holds the words, not React: each change goes straight to the matter's draft
  const keep = (text: string) => {
    if (matter === null) return;
    if (text === "") drafts.delete(matter);
    else drafts.set(matter, text);
  };
  // Enter comes here busy or empty too, and sends nothing then: it never breaks the line
  const ask = (asked: string) => {
    if (disabled || busy || asked.trim() === "") return;
    void onAsk(asked).then((ok) => {
      if (!ok) return;
      // words typed while it went keep their place; the emptied field's change drops the draft, and Cmd/Ctrl+Z
      // brings the question back
      if (field.current?.text() === asked) field.current.clear();
      // and an accepted question is no draft: the drafts outlive this composer, so this holds when the lawyer has
      // gone to another matter while the question went
      if (matter !== null && drafts.get(matter) === asked) drafts.delete(matter);
    });
  };
  const code = failed ?? refused;
  const working: WorkingState | null =
    code !== null
      ? {
          kind: "wait",
          words: errorWords(w.lang, code),
          pill: null,
          code,
          ...(failed !== null
            ? {
                actions: (
                  <Button
                    kind="quiet"
                    size="small"
                    className="working-act"
                    data-testid="retry"
                    onClick={onRetry}
                  >
                    <span className="deva-nudge">{lim(w.lang, "Try again")}</span>
                  </Button>
                ),
              }
            : {}),
        }
      : busy
        ? { kind: "wait", words: frame(w.lang, "Answering."), pill: null }
        : null;
  return (
    <Composer
      // keyed by the matter, so another matter's composer starts from that matter's draft
      key={matter ?? "none"}
      ref={field}
      took={null}
      plan=""
      hint={null}
      working={working}
      opening
      busy={busy}
      onStop={onStop}
      initialText={matter === null ? "" : (drafts.get(matter) ?? "")}
      onInput={keep}
      onAsk={ask}
      disabled={disabled}
      line={false}
      testIds={{
        field: "composer",
        ask: "send",
        stop: "cancel",
        working: code !== null ? "error" : "working",
      }}
    />
  );
}
