// Plans (UD2, settings.html#plans): one block per plan, Claude's then ChatGPT's, each its sign-in as UD1's parts say
// it, with what can be done from this window: sign out, stop using a reused login, look for one, add ChatGPT, sign in
// inline (the main window is not brought forward), and the other ways in (an API key, the firm's address). The state
// is the setup store's (store/setup.ts), over the window's own setup feed; every press is one setup command, which
// desktop's allow-list lets the settings window send (crates/desktop/src/setup/admit.rs). Nothing here keeps an API
// key, a pasted code or an address beyond its field, and nothing runs a timer.
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { setupStoreOf, CLAUDE, CODEX, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId } from "../../transport/protocol.ts";
import type { SectionContext } from "../sections.tsx";
import { PlanBlock } from "./plans-block.tsx";

/** The plans, in the design's order. */
const AGENTS: readonly AgentId[] = [CLAUDE, CODEX];

/** Whether a firm's policy locks the agent's plan: `plans.<id>`, or a prefix of it ending in `.` (`plans.`, while a
 * policy source can't be read; SettingsView.locked). */
const planLocked = (locked: readonly string[], agent: AgentId): boolean => {
  const key = `plans.${agent}`;
  return locked.some((k) => k === key || (k.endsWith(".") && key.startsWith(k)));
};

export function Plans({ ctx }: { ctx: SectionContext }) {
  const transport = ctx.transport as HostTransport & SetupSource;
  const [store] = useState(() => setupStoreOf(transport));
  useEffect(() => store.start(), [store]);
  const state = useSyncExternalStore(store.subscribe, store.state, store.state);
  const { platform } = useDisplay();
  // where a key is kept, by the computer's OS; null where it has no keyring (setup_info)
  const keys =
    state.keys === true
      ? platform === "mac" || platform === "windows"
        ? platform
        : "linux"
      : null;
  // "Use … instead" in a failed sign-in's words moves to the other plan's block
  const blocks = useRef(new Map<AgentId, HTMLElement>());
  const goTo = useCallback((agent: AgentId) => {
    const block = blocks.current.get(agent);
    block?.scrollIntoView?.({ block: "nearest" });
    block?.querySelector<HTMLElement>("button, input")?.focus();
  }, []);
  return (
    <div className="set-plans">
      {AGENTS.map((agent) => (
        <PlanBlock
          key={agent}
          agent={agent}
          setup={state.agents[agent]}
          keysKnown={state.keys !== null}
          keys={keys}
          store={store}
          transport={transport}
          locked={planLocked(ctx.settings.locked, agent)}
          endpoint={ctx.settings.plans?.[agent]?.endpoint}
          onOther={() => goTo(agent === CLAUDE ? CODEX : CLAUDE)}
          blockRef={(el) => {
            if (el === null) blocks.current.delete(agent);
            else blocks.current.set(agent, el);
          }}
        />
      ))}
    </div>
  );
}
