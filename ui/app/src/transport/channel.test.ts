import { afterEach, describe, expect, test } from "vitest";
import { ChannelReader, setTimedHook } from "./channel.ts";
import type { ChannelEvent } from "./channel.ts";
import type { HostEvent } from "./protocol.ts";

function reader() {
  const events: ChannelEvent[] = [];
  const problems: string[] = [];
  const r = new ChannelReader(
    (e) => events.push(e),
    (p) => problems.push(p),
  );
  return { r, events, problems };
}

const EVENT: HostEvent = {
  patches: {
    stream: "thread:x",
    epoch: "e",
    patches: [{ cursor: 1, body: { remove: { entry_id: "a" } } }],
  },
};

function pieces(text: string, n: number, id = 1) {
  const size = Math.ceil(text.length / n);
  return Array.from({ length: n }, (_, index) => ({
    fragment: { id, index, count: n, text: text.slice(index * size, (index + 1) * size) },
  }));
}

describe("ChannelReader", () => {
  test("a whole event as text, as bytes and as an object each arrive once and count one message", () => {
    const { r, events, problems } = reader();
    r.receive(JSON.stringify(EVENT));
    r.receive(new TextEncoder().encode(JSON.stringify(EVENT)).buffer);
    r.receive(EVENT);
    expect(events).toEqual([EVENT, EVENT, EVENT]);
    expect(r.messages).toBe(3);
    expect(problems).toEqual([]);
  });

  test("fragments join in index order into one event, each counting a message", () => {
    const { r, events } = reader();
    const parts = pieces(JSON.stringify(EVENT), 4);
    r.receive(JSON.stringify(parts[2]));
    r.receive(parts[0]!);
    r.receive(JSON.stringify(parts[3]));
    expect(events).toEqual([]);
    r.receive(JSON.stringify(parts[1]));
    expect(events).toEqual([EVENT]);
    expect(r.messages).toBe(4);
  });

  test("a newer fragment id drops an older unfinished event and says so", () => {
    const { r, events, problems } = reader();
    r.receive(pieces(JSON.stringify(EVENT), 2, 1)[0]!);
    for (const p of pieces(JSON.stringify(EVENT), 2, 2)) r.receive(p);
    expect(events).toEqual([EVENT]);
    expect(problems).toEqual(["fragment 1 never completed"]);
  });

  test("malformed messages are reported, not delivered", () => {
    const { r, events, problems } = reader();
    r.receive("{not json");
    r.receive({ fragment: { id: 1, index: 3, count: 2, text: "" } });
    r.receive({ fragment: { id: 2, index: 0, count: 1, text: "{broken" } });
    const dup = pieces(JSON.stringify(EVENT), 3, 3);
    r.receive(dup[0]!);
    r.receive(dup[0]!);
    expect(events).toEqual([]);
    expect(problems).toHaveLength(4);
    expect(r.messages).toBe(5);
  });

  test("an event split in fragments still joins while a timed hook is installed, with no time reported", () => {
    const heard: unknown[] = [];
    setTimedHook((e, ms) => heard.push([e, ms]));
    const { r, events } = reader();
    for (const p of pieces(JSON.stringify(EVENT), 3)) r.receive(p);
    setTimedHook(null);
    expect(events).toEqual([EVENT]);
    expect(heard).toEqual([]);
  });
});

describe("ChannelReader with timed messages (e2e builds)", () => {
  afterEach(() => setTimedHook(null));

  /** The bridge's wrapper: the event's JSON text spliced in raw. */
  const timed = (event: unknown, receivedMs: number) =>
    `{"timed":{"received_ms":${receivedMs},"event":${JSON.stringify(event)}}}`;

  test("a timed message is unwrapped: the event is handed on once, counts one message, then reaches the hook", () => {
    const order: string[] = [];
    const heard: [unknown, number][] = [];
    setTimedHook((e, ms) => {
      order.push("hook");
      heard.push([e, ms]);
    });
    const events: ChannelEvent[] = [];
    const r = new ChannelReader(
      (e) => {
        order.push("event");
        events.push(e);
      },
      () => {},
      () => order.push("message"),
    );
    r.receive(timed(EVENT, 1730380000123.456));
    expect(events).toEqual([EVENT]);
    expect(heard).toEqual([[EVENT, 1730380000123.456]]);
    expect(order).toEqual(["event", "hook", "message"]);
    expect(r.messages).toBe(1);
    expect(r.eventStart).toBe(0);
  });

  test("plain events still pass while the hook is installed, without reaching it", () => {
    const heard: unknown[] = [];
    setTimedHook((e) => heard.push(e));
    const { r, events } = reader();
    r.receive(JSON.stringify(EVENT));
    r.receive(timed(EVENT, 5));
    expect(events).toEqual([EVENT, EVENT]);
    expect(heard).toEqual([EVENT]);
    expect(r.messages).toBe(2);
  });

  test("without a hook (release) the reader does not look for the key: the message is handed on as it is", () => {
    const { r, events } = reader();
    r.receive(timed(EVENT, 5));
    expect(events).toEqual([{ timed: { received_ms: 5, event: EVENT } }]);
  });
});
