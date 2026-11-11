// Each plan's sign-in, as the setup wire says it (R20's SetupEvent, which store/setup.ts keeps for Plans): Plan and
// usage reads it so a plan that isn't set up is never shown as billed (D-060). Heard here straight off the wire, not
// through the setup store, so the store stays in Plans' lazy chunk and out of the window's first load.
import { useEffect, useState } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import type { AgentId, AgentKind, SetupEvent, SignIn } from "../transport/protocol.ts";

/** The plans' agent ids (store/setup.ts's CLAUDE and CODEX). */
const AGENT: Record<AgentKind, AgentId> = { claude: "claude-acp", codex: "codex-acp" };

type SetupFeed = { onSetup: (listener: (event: SetupEvent) => void) => () => void };

const carries = (transport: HostTransport): transport is HostTransport & SetupFeed =>
  typeof (transport as { onSetup?: unknown }).onSetup === "function";

/** Whether a sign-in state is a sign-in: a plan login, an API key or the firm's endpoint. */
const signedIn = (state: SignIn): boolean =>
  typeof state === "object" && "signed_in" in state && state.signed_in !== undefined;

/** Whether the setup wire has said the plan isn't signed in, in any way: false while it hasn't said, or where the
 * transport carries no setup wire. */
export type NotSetUp = (kind: AgentKind) => boolean;

/** The plans the setup wire says aren't signed in, live. */
export function useNotSetUp(transport: HostTransport): NotSetUp {
  const [states, setStates] = useState<Partial<Record<AgentId, SignIn>>>({});
  useEffect(() => {
    if (!carries(transport)) return;
    return transport.onSetup((event) => {
      const changed = "sign_in_changed" in event ? event.sign_in_changed : undefined;
      if (changed === undefined) return;
      setStates((was) => ({ ...was, [changed.agent]: changed.state }));
    });
  }, [transport]);
  return (kind) => {
    const state = states[AGENT[kind]];
    return state !== undefined && !signedIn(state);
  };
}
