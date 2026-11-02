// H13's Retry from here (`matters/retry`), at the end of one of the lawyer's questions: a new conversation from this
// question, with nothing deleted; the new one opens with retried-line.tsx's line. It reads the session's retry
// (session.ts's ThreadActions), so it draws nothing where the host can't retry. It shows under the pointer or with
// focus, as the design's does. The question row places it (thread/rows.tsx's Question, for live questions only);
// until that row carries it, nothing renders it but the copy review.
import { useState } from "react";
import { IconUndo } from "@clauseo/components/icons/icons.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { MessageId, Timestamp } from "../transport/protocol.ts";
import { useThreadActions } from "./thread/session.ts";

export function RetryFromHere({
  message,
  at,
  shown = false,
}: {
  message: MessageId;
  at: Timestamp;
  /** Shown at rest too, not only under the pointer or with focus: for the copy review. */
  shown?: boolean;
}) {
  const { lang } = useWords();
  const { retry } = useThreadActions();
  const [going, setGoing] = useState(false);
  if (retry === undefined) return null;
  return (
    <Button
      kind="quiet"
      size="small"
      className={
        shown ? "mt-s1" : "mt-s1 opacity-0 focus-visible:opacity-100 [:hover>&]:opacity-100"
      }
      title={mt(
        lang,
        "Retry from here starts a new conversation from this question. Nothing is deleted.",
      )}
      aria-disabled={going || undefined}
      data-testid="retry-from-here"
      onClick={() => {
        if (going) return;
        setGoing(true);
        retry(message, at)
          .catch((error: unknown) => console.warn(`retry_from failed: ${JSON.stringify(error)}`))
          .finally(() => setGoing(false));
      }}
    >
      <IconUndo size={16} />
      <span className="deva-nudge">{mt(lang, "Retry from here")}</span>
    </Button>
  );
}
