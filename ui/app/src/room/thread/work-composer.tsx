// The room's composer as the thread drives it: Ask, or Stop while a turn runs; Enter sends (composer-keys.ts's
// sendsOnEnter: Shift+Enter breaks the line, and an input method's composing Enter is its own); the field keeps its
// words until the host accepts the question, and after a refusal. The working line says "Answering." while busy, and
// stands for an error by its code: a lost stream, with Try again, before a refused command.
import { useEffect, useState, type KeyboardEvent } from "react";
import { Composer } from "@clauseo/components/clauseo/composer.tsx";
import { sendsOnEnter } from "@clauseo/components/clauseo/composer-keys.ts";
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
  const [text, setText] = useState(() => (matter === null ? "" : (drafts.get(matter) ?? "")));
  useEffect(() => {
    if (matter === null) return;
    if (text === "") drafts.delete(matter);
    else drafts.set(matter, text);
  }, [drafts, matter, text]);
  const ask = () => {
    if (disabled || busy || text.trim() === "") return;
    const asked = text;
    void onAsk(asked).then((ok) => {
      // words typed while it went keep their place
      if (ok) setText((now) => (now === asked ? "" : now));
    });
  };
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    const key = {
      key: e.key,
      shiftKey: e.shiftKey,
      isComposing: e.nativeEvent.isComposing,
      keyCode: e.keyCode,
    };
    if (!sendsOnEnter(key)) return false;
    // taken while busy or empty too: Enter never breaks the line, it only sends
    ask();
    return true;
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
      took={null}
      plan=""
      hint={null}
      working={working}
      opening
      busy={busy}
      onStop={onStop}
      text={text}
      onText={setText}
      onKeyCommand={onKey}
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
