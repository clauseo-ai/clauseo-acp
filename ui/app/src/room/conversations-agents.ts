// What conversations.ts knows of the agents, apart from it to keep that file under the size limit: R15's list_agents,
// read at start and on each settings_changed, so a plan set up while the room is open joins the choice and its options
// (the model and effort it last reported) reach the draft; the plans signed in and the plan last asked with, for the
// default plan (plan-default.ts); and a draft's model and effort picks, which create_thread sends as its config
// (pickers.ts).
import type { HostTransport } from "../transport/host-transport.ts";
import type { AgentId, AgentKind, ConfigPicks, MatterSummary } from "../transport/protocol.ts";
import { agentFor, kindOf, planOptions, type Used } from "./agent.ts";
import { hasSetup } from "./first-run/hold.ts";
import type { AgentSummary, NextHost } from "./host-next.ts";
import { lastPlan, SignedPlans } from "./plan-default.ts";
import { draftConfig } from "./pickers.ts";

/** A draft's model and effort picks, by the plan's kind: switching plans keeps each plan's own. */
export type DraftPicks = Partial<Record<AgentKind, ConfigPicks>>;
export const NO_PICKS: DraftPicks = {};

export class AgentList {
  readonly #transport: HostTransport;
  readonly #next: NextHost;
  readonly #changed: () => void;
  readonly #live: () => boolean;
  /** Whether the host carries the setup wire: then both plans are offered on a draft, set up or not. */
  readonly #setup: boolean;
  readonly #plans = new SignedPlans(); // the plans signed in (plan-default.ts), for the default plan
  #reads = 0; // the list_agents reads sent: a reply a later read overtook is dropped
  agents: AgentSummary[] = [];

  constructor(transport: HostTransport, next: NextHost, changed: () => void, live: () => boolean) {
    this.#transport = transport;
    this.#next = next;
    this.#changed = changed;
    this.#live = live;
    this.#setup = hasSetup(transport);
  }

  /** Reads the agents now and on each settings_changed, and hears the sign-ins; returns the stops. */
  start(): (() => void)[] {
    const stops: (() => void)[] = [];
    // a plan set up while the room is open writes its agents entry, and the host says settings_changed; so does a
    // change in the options an agent reported, which the drafts' pickers read
    const settings = this.#next.settings;
    if (settings !== null && this.#next.agents !== null)
      stops.push(settings.onSettingsChanged(() => this.#list()));
    this.#list();
    stops.push(this.#plans.watch(this.#transport, this.#changed));
    return stops;
  }

  /** R15's list_agents; the latest read's answer wins. */
  #list(): void {
    const agents = this.#next.agents;
    if (agents === null) return;
    const at = (this.#reads += 1);
    agents.listAgents().then(
      (list) => {
        if (!this.#live() || at !== this.#reads) return;
        this.agents = list;
        this.#changed();
      },
      (error: unknown) => console.warn(`list_agents failed: ${JSON.stringify(error)}`),
    );
  }

  /** The agent a new thread runs: the draft's choice, else agent.ts's order. */
  agentFor(chosen: AgentId | null, threads: readonly Used[]): AgentId {
    const [agents, signed, last] = [this.agents, this.#plans.signed, lastPlan()];
    return agentFor({ chosen, threads, agents, signed, last });
  }

  /** The draft's plan choice: both plans where the host can set one up from the room, else the plans listed. */
  options(threads: readonly Used[] = []): { value: AgentId; label: string }[] {
    return planOptions(this.agents, threads, this.#setup);
  }

  /** The kind of an agent, for its remembered and picked values. */
  kindOf(agent: AgentId): AgentKind | undefined {
    return kindOf(agent, this.agents);
  }

  /** What create_thread sends as `config` for a draft of `agent`: undefined while the lawyer picked nothing. */
  configFor(agent: AgentId, picks: DraftPicks | undefined, matter: MatterSummary | null) {
    const kind = this.kindOf(agent);
    if (kind === undefined) return undefined;
    const options = this.agents.find((a) => a.id === agent)?.config_options;
    return draftConfig(options, picks?.[kind], matter?.agent_prefs?.[kind]);
  }
}
