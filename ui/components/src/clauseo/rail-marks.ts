// Each rail status's icon (rail-mark.tsx draws it): yellow, as a disc, only where the matter or the conversation waits
// on the lawyer's hand. Failed takes the warning triangle in concrete: no red, and no yellow, for a failure.
import type { ComponentType } from "react";
import type { IconProps } from "../icons/icon.tsx";
import {
  IconAnswerReady,
  IconChecking,
  IconFailed,
  IconLocateFolder,
  IconNeedsPermission,
  IconPlanLimit,
  IconQuestion,
  IconSnooze,
  IconStop,
  IconTick,
} from "../icons/icons.tsx";
import type { ConversationState, MatterState } from "./rail-words.ts";

export type Mark = { icon: ComponentType<IconProps>; waits: boolean };

/** Each matter status's mark: yellow only where the matter waits for the lawyer's hand. The plan's limit, waiting on the
 * reset or on the lawyer's choice, takes the gauge, not the clock, so it reads apart from an answer under way; working
 * and a conversation waiting behind another share the clock. */
export const MATTER_MARK: Record<Exclude<MatterState, "idle">, Mark> = {
  question: { icon: IconQuestion, waits: true },
  permission: { icon: IconNeedsPermission, waits: true },
  paused: { icon: IconPlanLimit, waits: true },
  ready: { icon: IconAnswerReady, waits: false },
  working: { icon: IconChecking, waits: false },
  stopped: { icon: IconStop, waits: false },
  pausedSet: { icon: IconPlanLimit, waits: false },
  limit: { icon: IconPlanLimit, waits: false },
  moved: { icon: IconLocateFolder, waits: false },
  failed: { icon: IconFailed, waits: false },
};

/** Each conversation status's mark, the same shapes as the matters'. */
export const CONVERSATION_MARK: Record<Exclude<ConversationState, "idle">, Mark> = {
  question: { icon: IconQuestion, waits: true },
  permission: { icon: IconNeedsPermission, waits: true },
  ready: { icon: IconAnswerReady, waits: false },
  working: { icon: IconChecking, waits: false },
  stopped: { icon: IconStop, waits: false },
  waiting: { icon: IconChecking, waits: false },
  failed: { icon: IconFailed, waits: false },
};

/** Snoozed (U16b): the alarm clock, in the line's own tone. */
export const SNOOZED: Mark = { icon: IconSnooze, waits: false };

/** Done (U16b): the tick, on the open conversation once done. */
export const DONE: Mark = { icon: IconTick, waits: false };
