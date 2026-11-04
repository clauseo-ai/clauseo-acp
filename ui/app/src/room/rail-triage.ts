// The rail's triage over time: triage.ts's pure placement, memoised on its inputs' identity, with what it can't know
// alone. Rows don't change group while the pointer is over the rail, and do at pointer leave; the open matter stays in
// the group it stood in when it opened until another opens; Needs you remembers the order its rows came in. No
// timers, no animation: a change lands with the next render after the pointer leaves. The pointer is an input like the
// rest, so a render memoised on the inputs (the React Compiler's) still sees it change.
import type { MatterId, ThreadSummary } from "../transport/protocol.ts";
import type { MatterInfo } from "./host-next.ts";
import type { RowStatus } from "./matters.ts";
import { triage, type Arrival, type Group, type Triage } from "./triage.ts";

type Inputs = {
  matters: readonly MatterInfo[];
  status: ReadonlyMap<MatterId, RowStatus>;
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>;
  open: MatterId | null;
  /** Whether the pointer is over the rail. */
  over: boolean;
};

const NONE: ReadonlyMap<MatterId, Arrival> = new Map();

export class RailTriage {
  #last: (Inputs & { result: Triage }) | null = null;
  #arrived: ReadonlyMap<MatterId, Arrival> = NONE;
  /** The groups as they stood when the pointer came over the rail; null while it is elsewhere. */
  #frozen: ReadonlyMap<MatterId, Group> | null = null;
  /** The open matter and the group it opened in. */
  #open: { matter: MatterId; group: Group } | null = null;

  /** The triage for these inputs: the last one again when nothing changed. */
  of(inputs: Inputs): Triage {
    const last = this.#last;
    if (
      last !== null &&
      last.matters === inputs.matters &&
      last.status === inputs.status &&
      last.lists === inputs.lists &&
      last.open === inputs.open &&
      last.over === inputs.over
    )
      return last.result;
    // the pointer came over the rail: every row keeps the group it stands in until it leaves
    if (inputs.over && this.#frozen === null) this.#frozen = last?.result.group ?? new Map();
    if (!inputs.over) this.#frozen = null;
    if (inputs.open !== (this.#open?.matter ?? null)) {
      // the matter opens where it stood: as last drawn, else where it falls now
      const stood = inputs.open === null ? undefined : last?.result.group.get(inputs.open);
      this.#open =
        inputs.open === null
          ? null
          : {
              matter: inputs.open,
              group: stood ?? this.#place(inputs, null).group.get(inputs.open) ?? "date",
            };
    }
    const held = new Map(this.#frozen ?? []);
    if (this.#open !== null) held.set(this.#open.matter, this.#open.group);
    const result = this.#place(inputs, held);
    this.#arrived = result.arrived;
    this.#last = { ...inputs, result };
    return result;
  }

  #place(inputs: Inputs, held: ReadonlyMap<MatterId, Group> | null): Triage {
    return triage({
      matters: inputs.matters,
      status: inputs.status,
      lists: inputs.lists,
      held,
      arrived: this.#arrived,
    });
  }
}
