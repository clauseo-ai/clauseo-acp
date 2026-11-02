// The line above the field while the lawyer types during an answer (U7, matters/steer-claude and steer-chatgpt): what
// Send now does with this agent, and Send now and Queue at its right end. In a file of its own, so the room's first load
// carries it without the queue's strip.
import { Button } from "../ui/button.tsx";
import { useMatterWords } from "../lib/words-matters.ts";
import { Say } from "./say.tsx";

/** The line above the field while the lawyer types during an answer: what Send now does with this plan, and at its
 * right end Send now and Queue, two quiet buttons, so the field keeps its width. Enter still queues. */
export function SteerHint({
  text,
  onSendNow,
  onQueue,
  full = false,
}: {
  text: string;
  onSendNow: () => void;
  onQueue: () => void;
  full?: boolean;
}) {
  const w = useMatterWords();
  return (
    <div className="steer-hint">
      <p className="steer-hint-text">
        <Say>{text}</Say>
      </p>
      <span className="steer-hint-acts">
        <Button kind="quiet" size="small" className="mat-send-now" onClick={onSendNow}>
          <span className="deva-nudge">{w.m("Send now")}</span>
        </Button>
        <Button
          kind="quiet"
          size="small"
          className="mat-queue"
          aria-keyshortcuts="Enter"
          disabled={full}
          onClick={onQueue}
        >
          <span className="deva-nudge">{w.m("Queue")}</span>
        </Button>
      </span>
    </div>
  );
}
