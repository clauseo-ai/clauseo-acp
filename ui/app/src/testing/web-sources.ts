// Envelopes of the web sources' two domain events, for tests and the copy review (room/thread/sources.ts): a search
// and a page, on THREAD's stream and TURN unless told otherwise, and the channel's `events` arm that carries them.
import type {
  AgentId,
  Envelope,
  HostEvent,
  PageVia,
  StreamId,
  TurnId,
  WebResult,
} from "../transport/protocol.ts";
import { STREAM, TURN, uuid } from "./stream-script.ts";

type Where = { stream?: StreamId; turn?: TurnId };

const envelope = (seq: number, stream: StreamId, event: Envelope["event"]): Envelope => ({
  schema_version: 1,
  event_id: uuid(seq, 30),
  stream,
  seq,
  ts: 1_700_000_000_000 + seq,
  event,
});

/** A web search by `agent` (claude-acp by default), with its results. */
export const searched = (
  seq: number,
  query: string,
  results: WebResult[] = [],
  { agent = "claude-acp", stream = STREAM, turn = TURN }: Where & { agent?: AgentId } = {},
): Envelope =>
  envelope(seq, stream, {
    web_search_recorded: { turn, tool_call: `call-${seq}`, agent, query, results },
  });

/** A page the agent opened, `via` web_fetch by default, with its title when it has one. */
export const visited = (
  seq: number,
  url: string,
  {
    title,
    via = "web_fetch",
    stream = STREAM,
    turn = TURN,
  }: Where & { title?: string; via?: PageVia } = {},
): Envelope =>
  envelope(seq, stream, {
    page_visited: {
      turn,
      tool_call: `call-${seq}`,
      url,
      via,
      ...(title === undefined ? {} : { title }),
    },
  });

/** The channel message that carries `envelopes`. */
export const eventsOf = (envelopes: Envelope[]): HostEvent => ({ events: envelopes });
