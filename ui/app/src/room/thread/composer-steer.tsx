// The composer's line above the field while Clauseo works and the field holds words (U7, matters/steer-claude and
// steer-chatgpt): what Enter and Send now do with this agent, then Send now and Queue. Where the agent takes words
// mid-answer (H9's `steer_joins_turn`, claude-agent-acp's) Enter adds them to the running answer, else Enter sends them
// once the agent finishes; Send now stops the answer and sends them, on every agent. The agent is named as the plan
// line names it (limit-words.ts planNameOf: Claude, ChatGPT for Codex, a kindless agent's own name, Claude while the
// room doesn't know). Queue queues the words whatever Enter does, and is disabled once the queue holds QUEUE_CAP. Each
// read is a boolean or a string, so a streamed chunk or a keystroke renders nothing here.
import { SteerHint } from "@clauseo/components/clauseo/steer-hint.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamId } from "../../transport/protocol.ts";
import { useAgentPlan } from "./agent-plan.ts";
import { joinsIn, pickFull } from "./composer-queue.ts";
import { planNameOf } from "./limit-words.ts";

const TEST_IDS = { row: "steer-hint", sendNow: "steer-send-now", queue: "queue" } as const;

export function ComposerSteer({
  stream,
  onSendNow,
  onQueue,
}: {
  stream: StreamId | null;
  /** Send now: the field's words with `delivery: "send_now"`. */
  onSendNow: () => void;
  /** Queue: the field's words, queued. */
  onQueue: () => void;
}) {
  const { lang } = useWords();
  const store = useViewStore();
  const joins = useStreamPart(store, stream, joinsIn);
  const full = useStreamPart(store, stream, pickFull);
  const name = planNameOf(useAgentPlan());
  const text = joins
    ? mt(lang, "Enter adds your words to this answer. Send now stops the answer and sends them.")
    : mt(
        lang,
        "Enter sends your words when {name} finishes. Send now stops the answer and sends them.",
        {
          name,
        },
      );
  return (
    <SteerHint
      text={text}
      onSendNow={onSendNow}
      onQueue={onQueue}
      full={full}
      queueOnEnter={!joins}
      testIds={TEST_IDS}
    />
  );
}
