import { describe, expect, test } from "vitest";
import { ChannelReader } from "./channel.ts";
import type { HostEvent } from "./protocol.ts";

function reader() {
  const events: HostEvent[] = [];
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
  test("a whole event as text, as bytes and as an object each arrive once and count one frame", () => {
    const { r, events, problems } = reader();
    r.receive(JSON.stringify(EVENT));
    r.receive(new TextEncoder().encode(JSON.stringify(EVENT)).buffer);
    r.receive(EVENT);
    expect(events).toEqual([EVENT, EVENT, EVENT]);
    expect(r.frames).toBe(3);
    expect(problems).toEqual([]);
  });

  test("fragments join in index order into one event, each counting a frame", () => {
    const { r, events } = reader();
    const parts = pieces(JSON.stringify(EVENT), 4);
    r.receive(JSON.stringify(parts[2]));
    r.receive(parts[0]!);
    r.receive(JSON.stringify(parts[3]));
    expect(events).toEqual([]);
    r.receive(JSON.stringify(parts[1]));
    expect(events).toEqual([EVENT]);
    expect(r.frames).toBe(4);
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
    expect(r.frames).toBe(5);
  });
});
