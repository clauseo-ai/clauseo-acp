// Plans (UD2, settings.html#plans): one block per plan, Claude's then ChatGPT's, each its sign-in as UD1's parts say
// it, with what can be done from this window: sign out, stop using a reused login, look for one, add a plan, sign in
// inline (the main window is not brought forward), and the other ways in (an API key, the firm's address); under the
// cards, Clauseo's document tools getting ready, on one quiet line. The state
// is the setup store's (store/setup.ts), over the window's own setup feed; every press is one setup command, which
// desktop's allow-list lets the settings window send (crates/desktop/src/setup/admit.rs). Nothing here keeps an API
// key, a pasted code or an address beyond its field, and nothing runs a timer.
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import {
  setupStoreOf,
  CLAUDE,
  CODEX,
  isReady,
  isSignedIn,
  planLocked,
  type SetupSource,
  type SetupState,
} from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId } from "../../transport/protocol.ts";
import type { SectionContext } from "../sections.tsx";
import { ToolsLine } from "@clauseo/components/clauseo/first-launch/tools.tsx";
import { readyPlan, toolsView } from "../../room/first-run/views.ts";
import { PlanBlock } from "./plans-block.tsx";

/** The plans, in the design's order. */
const AGENTS: readonly AgentId[] = [CLAUDE, CODEX];

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
  // a plan the lawyer can switch to from here: installed (or the firm's own install) or signed in, and not held by
  // the firm's policy; "Use … instead" names no other plan
  const usable = (agent: AgentId) => {
    const setup = state.agents[agent];
    return (
      !planLocked(ctx.settings.locked, agent) &&
      (isReady(setup?.install) || setup?.install === "managed" || isSignedIn(setup?.signIn))
    );
  };
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
          otherReady={usable(agent === CLAUDE ? CODEX : CLAUDE)}
          onOther={() => goTo(agent === CLAUDE ? CODEX : CLAUDE)}
          blockRef={(el) => {
            if (el === null) blocks.current.delete(agent);
            else blocks.current.set(agent, el);
          }}
        />
      ))}
      {/* the document tools, one quiet line under the plan cards (the first screen's own) */}
      <PlansTools
        state={state}
        retry={(agent) => store.command({ retry_agent_install: { agent } })}
      />
    </div>
  );
}

function PlansTools({ state, retry }: { state: SetupState; retry: (agent: AgentId) => void }) {
  const view = toolsView(state);
  if (view === null) return null;
  const through = readyPlan(state);
  return (
    <div className="set-plans-tools">
      <ToolsLine
        view={view}
        onRetry={through === null ? undefined : () => retry(through)}
        testId="plans-tools"
      />
    </div>
  );
}
