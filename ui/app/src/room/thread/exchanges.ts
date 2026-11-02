// The thread's exchanges, by turn (U7). A queued message keeps the order it was sent at when its turn later starts
// (H9: only its status flips to sent), so it sits among the earlier turn's entries; grouping by position would draw
// that turn's tail under it. Here each sent question heads the exchange of its turn, every entry with a turn joins
// that turn's exchange, and an entry without one (a notice, a permission) joins the exchange still running where it
// arrived, else the newest. Queued and removed messages show in no exchange: they stand in the queue's strip. A steer
// taken into the running answer (or not yet resolved while a turn runs) is no question: it joins the running answer as
// a mark where it arrived (steer.ts).
//
// The live list reads this through one pick (exchangesPick) that builds the grouping again only when the id list
// changes, a turn starts or stops running, or a user message's status, turn or steer changes; an edit of a queued
// message's words changes none of those, so it renders no exchange and no row.
import { turnRunning } from "./live.ts";
import { askedAs, injectedTurn, outcomeOf, shapeOf } from "./steer.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, TurnId, ViewEntry } from "../../transport/protocol.ts";
import type { UserMessageEntryH9 } from "../../transport/h9-pending.ts";

/** One exchange: its ids in order, the question first when it has one. */
export type Exchange = {
  /** React's key: the question's id, or the first id of an exchange without one. */
  readonly key: EntryId;
  /** Whether the first id is the question that heads it (a window that opens mid-turn has none). */
  readonly asked: boolean;
  readonly ids: readonly EntryId[];
  /** Its step's id, when a steer sent now stopped this turn: the step says so in place of its stop line. */
  readonly sentNow: EntryId | null;
};

type Building = {
  key: EntryId;
  asked: boolean;
  turn: TurnId | undefined;
  ids: EntryId[];
  /** Where its first entry other than the question stands: exchanges go in the order their turns began. */
  first: number;
  /** Where it was opened, to order two that have not begun. */
  at: number;
  stopped: boolean;
};

const turnOf = (e: ViewEntry): TurnId | undefined =>
  e.agent_text?.turn ??
  e.tool_call?.turn ??
  e.agent_content?.turn ??
  e.step?.turn ??
  e.limit_stop?.turn;

const STEP = "step:";

/** The exchanges of entries in order. `running`: whether a turn runs, for a steer not yet resolved. */
export function groupExchanges(
  items: readonly (readonly [EntryId, ViewEntry | undefined])[],
  running: boolean,
): Exchange[] {
  const out: Building[] = [];
  const byTurn = new Map<TurnId, Building>();
  const ended = new Set<TurnId>();
  // the exchange of the latest entry with a turn: where an entry without one arrived
  let open: Building | null = null;
  const opened = (key: EntryId, asked: boolean, turn: TurnId | undefined, at: number) => {
    const ex: Building = {
      key,
      asked,
      turn,
      ids: [],
      first: asked ? Infinity : at,
      at,
      stopped: false,
    };
    out.push(ex);
    if (turn !== undefined) byTurn.set(turn, ex);
    return ex;
  };
  const turned = (id: EntryId, turn: TurnId, at: number) => {
    const known = byTurn.get(turn);
    // a turn with no exchange: the window opened mid-turn (its question is on an older page) when nothing is before
    // it; after an exchange, it is placed as an entry without a turn is
    if (known === undefined && out.length > 0) return unturned(id, at);
    const ex = known ?? opened(id, false, turn, at);
    ex.ids.push(id);
    if (ex.first === Infinity) ex.first = at;
    open = ex;
  };
  const unturned = (id: EntryId, at: number) => {
    const live = open !== null && (open.turn === undefined || !ended.has(open.turn)) ? open : null;
    const ex = live ?? out.at(-1) ?? opened(id, false, undefined, at);
    ex.ids.push(id);
    if (ex.first === Infinity) ex.first = at;
  };
  items.forEach(([id, e], at) => {
    if (e === undefined) return;
    const m = e.user_message as UserMessageEntryH9 | undefined;
    if (m !== undefined) {
      const as = askedAs(m, running);
      if (as === "hidden") return;
      if (as === "mark") {
        const into = injectedTurn(m);
        if (into === undefined) unturned(id, at);
        else turned(id, into, at);
        return;
      }
      // a steer sent now stopped the turn that ran as it arrived
      if (outcomeOf(m) === "sent_now" && open !== null) open.stopped = true;
      const again = byTurn.has(m.turn);
      const ex = opened(id, true, m.turn, at);
      ex.ids.push(id);
      // a second question on one turn (only made-up views have one) takes what follows, as by position
      if (again) open = ex;
      return;
    }
    const turn = turnOf(e);
    if (turn === undefined) return unturned(id, at);
    turned(id, turn, at);
    if (e.step !== undefined || e.limit_stop !== undefined) ended.add(turn);
  });
  out.sort((a, b) => (a.first === b.first ? a.at - b.at : a.first < b.first ? -1 : 1));
  return out.map((ex) => ({
    key: ex.key,
    asked: ex.asked,
    ids: ex.ids,
    sentNow: ex.stopped ? (ex.ids.find((id) => id.startsWith(STEP)) ?? null) : null,
  }));
}

/** The live list's shape: its exchanges, and the window's first id (where older pages end). */
export type Shape = { readonly exchanges: readonly Exchange[]; readonly first: EntryId | null };

const EMPTY: Shape = { exchanges: [], first: null };

const USER = "user_message:";
const userIdsOf = new WeakMap<readonly EntryId[], readonly EntryId[]>();
function userIds(ids: readonly EntryId[]): readonly EntryId[] {
  let own = userIdsOf.get(ids);
  if (own === undefined) {
    own = ids.filter((id) => id.startsWith(USER));
    userIdsOf.set(ids, own);
  }
  return own;
}

const sameExchange = (a: Exchange, b: Exchange) =>
  a.key === b.key &&
  a.asked === b.asked &&
  a.sentNow === b.sentNow &&
  a.ids.length === b.ids.length &&
  a.ids.every((id, i) => id === b.ids[i]);

/** A pick of the live list's shape, the same object while it reads the same: one per mount (useState(exchangesPick)).
 * A flush that only appends text reads the id list's identity and each user message's entry object, no more. */
export function exchangesPick(): (v: StreamView | undefined) => Shape {
  let ids: readonly EntryId[] | null = null;
  let running = false;
  let seen: (ViewEntry | undefined)[] = [];
  let shapes: string[] = [];
  let last = EMPTY;
  return (v) => {
    if (v === undefined) {
      ids = null;
      return (last = EMPTY);
    }
    const run = turnRunning(v.header?.header?.status);
    const users = userIds(v.ids);
    let stale = v.ids !== ids || run !== running;
    if (!stale)
      for (let i = 0; i < users.length; i += 1) {
        const e = v.entries.get(users[i]!);
        if (e === seen[i]) continue;
        seen[i] = e;
        const shape = shapeOf(e?.user_message as UserMessageEntryH9 | undefined);
        if (shape !== shapes[i]) {
          shapes[i] = shape;
          stale = true;
        }
      }
    if (!stale) return last;
    ids = v.ids;
    running = run;
    seen = users.map((id) => v.entries.get(id));
    shapes = seen.map((e) => shapeOf(e?.user_message as UserMessageEntryH9 | undefined));
    const built = groupExchanges(
      v.ids.map((id) => [id, v.entries.get(id)] as const),
      run,
    );
    // keep each exchange that reads the same, so its memoized rows don't render
    const before = new Map(last.exchanges.map((o) => [o.key, o]));
    const exchanges = built.map((ex) => {
      const o = before.get(ex.key);
      return o !== undefined && sameExchange(o, ex) ? o : ex;
    });
    const first = v.ids[0] ?? null;
    if (
      first === last.first &&
      exchanges.length === last.exchanges.length &&
      exchanges.every((ex, i) => ex === last.exchanges[i])
    )
      return last;
    return (last = exchanges.length === 0 && first === null ? EMPTY : { exchanges, first });
  };
}

/** The question heading the newest exchange, or null. */
export const latestAsked = (shape: Shape): EntryId | null => {
  for (let i = shape.exchanges.length - 1; i >= 0; i -= 1)
    if (shape.exchanges[i]!.asked) return shape.exchanges[i]!.key;
  return null;
};
