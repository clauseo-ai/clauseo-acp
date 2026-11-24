// The assistant the place's words name: "Claude" or "ChatGPT" when one plan is signed in, "the
// assistant" when both are (orchestrator rule, 24 Nov: name the one in use, never a made-up
// neutral). Before any sign-in state is heard, or with none signed in, the plan the room's new
// conversations would run names it.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { useEffect, useState } from "react";
import { SignedPlans, type Signed } from "../room/plan-default.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { AgentKind } from "../transport/protocol.ts";

const PLAN: Record<AgentKind, string> = { claude: "Claude", codex: "ChatGPT" };

export type Assistant = {
  /** The plan's name, or null when both are signed in. */
  name: string | null;
  /** Mid-sentence. */
  who: string;
  /** At a sentence's start. */
  Who: string;
};

/** The name from the plans signed in, else from the room's default plan. */
export function assistantName(signed: Signed, fallback: AgentKind | undefined): string | null {
  if (signed !== null && signed.size > 1) return null;
  const one = signed === null ? undefined : [...signed][0];
  return PLAN[one ?? fallback ?? "claude"];
}

/** The assistant, as the sign-ins change. */
export function useAssistant(transport: HostTransport, fallback: AgentKind | undefined): Assistant {
  const { lang } = useDisplay();
  const [signed, setSigned] = useState<Signed>(null);
  useEffect(() => {
    const plans = new SignedPlans();
    return plans.watch(transport, () => setSigned(plans.signed));
  }, [transport]);
  const name = assistantName(signed, fallback);
  return {
    name,
    who: name ?? lb(lang, "the assistant"),
    Who: name ?? lb(lang, "The assistant"),
  };
}
