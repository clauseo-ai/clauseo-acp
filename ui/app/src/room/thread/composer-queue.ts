// The queue's cap as the composer meets it (U7, matters/queue-full): whether the queue holds QUEUE_CAP, when Queue takes
// no press and Enter queues nothing, and the line a send refused with `thread.queue_full` shows. And whether the agent
// takes words mid-answer, which decides what Enter does while an answer runs.
import { QUEUE_CAP } from "@clauseo/components/clauseo/queue-model.ts";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import type { StreamView } from "../../store/view.ts";
import { queueCountPick } from "../queue-entries.ts";

/** Whether the queue holds QUEUE_CAP: a boolean, so a pick of it renders only as it flips. */
export const pickFull = (v: StreamView | undefined): boolean => queueCountPick(v) >= QUEUE_CAP;

/** The cap line ("The queue holds 10."), on the composer's working line in place of the code's words. */
export const queueFullLine = (lang: Lang): string =>
  mt(lang, "The queue holds {n}.", { n: QUEUE_CAP });

/** H9's steering flag on the thread's header, true only once the host says the agent takes words into a running
 * answer (claude-agent-acp's): Enter steers then, else queues. A boolean, so a pick of it renders only as it flips. */
export const joinsIn = (v: StreamView | undefined): boolean =>
  v?.header?.header?.steer_joins_turn === true;
