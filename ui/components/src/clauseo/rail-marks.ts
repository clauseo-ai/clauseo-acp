// Each rail status's icon (rail-mark.tsx draws it): yellow, as a disc, only where the matter or the conversation waits
// on the lawyer's hand.
import type { ComponentType } from "react";
import type { IconProps } from "../icons/icon.tsx";
import {
  IconAnswerReady,
  IconChecking,
  IconLocateFolder,
  IconNeedsPermission,
  IconPlanLimit,
  IconQuestion,
  IconStop,
} from "../icons/icons.tsx";
import type { ConversationState, MatterState } from "./rail-words.ts";

export type Mark = { icon: ComponentType<IconProps>; waits: boolean };

/** Each matter status's mark: yellow only where the matter waits for the lawyer's hand. */
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
};

/** Each conversation status's mark, the same shapes as the matters'. */
export const CONVERSATION_MARK: Record<Exclude<ConversationState, "idle">, Mark> = {
  question: { icon: IconQuestion, waits: true },
  ready: { icon: IconAnswerReady, waits: false },
  working: { icon: IconChecking, waits: false },
  stopped: { icon: IconStop, waits: false },
  waiting: { icon: IconChecking, waits: false },
};
