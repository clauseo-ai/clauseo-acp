// The two ways on from a break, on the working line (thread-break.ts): Restart, once the host restarts an agent (H10),
// and Try again after a stream that failed.
import { IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { Button } from "@clauseo/components/ui/button.tsx";

export function RestartButton({ lang, onRestart }: { lang: Lang; onRestart: () => void }) {
  return (
    <Button
      kind="tone"
      size="small"
      className="working-act"
      data-testid="restart"
      onClick={onRestart}
    >
      <IconTryAgain size={16} />
      <span className="deva-nudge">{th(lang, "Restart")}</span>
    </Button>
  );
}

export function RetryButton({ lang, onRetry }: { lang: Lang; onRetry: () => void }) {
  return (
    <Button kind="quiet" size="small" className="working-act" data-testid="retry" onClick={onRetry}>
      <IconTryAgain size={16} />
      <span className="deva-nudge">{lim(lang, "Try again")}</span>
    </Button>
  );
}
