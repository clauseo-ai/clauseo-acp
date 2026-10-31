import { describe, expect, test } from "vitest";
import { fakeClock } from "./fake-clock.ts";
import { watchMarks } from "./marks.ts";

const settle = () => new Promise((done) => setTimeout(done, 0));

describe("watchMarks", () => {
  test("interactive: the frame after the one in which the room is rendered and the channel registered", async () => {
    const f = fakeClock(1000);
    let rendered = false;
    let connect = () => {};
    const connected = new Promise<void>((done) => (connect = done));
    const marks = watchMarks({
      t0EpochMs: 900,
      clock: f.clock,
      paintEntries: () => [],
      rendered: () => rendered,
      connected,
    });
    f.frame(10); // 10
    rendered = true;
    f.frame(10); // 20: rendered, not connected
    connect();
    await settle();
    expect(marks().interactive_ms).toBeNull();
    f.frame(10); // 30: both seen
    expect(marks().interactive_ms).toBeNull();
    f.frame(16); // 46: that frame was painted
    expect(marks().interactive_ms).toBe(1000 + 46 - 900);
    f.frame();
    expect(marks().interactive_ms).toBe(146);
  });

  test("first paint: first-paint, else first-contentful-paint, else the double rAF after watching began", () => {
    const f = fakeClock(1000);
    let entries: { name: string; startTime: number }[] = [];
    const marks = watchMarks({
      t0EpochMs: 900,
      clock: f.clock,
      paintEntries: () => entries,
      rendered: () => false,
      connected: new Promise(() => {}),
    });
    expect(marks().first_paint_ms).toBeNull();
    f.frame(5);
    f.frame(5);
    expect(marks().first_paint_ms).toBe(110);
    entries = [{ name: "first-contentful-paint", startTime: 40 }];
    expect(marks().first_paint_ms).toBe(140);
    entries = [...entries, { name: "first-paint", startTime: 30 }];
    expect(marks().first_paint_ms).toBe(130);
  });
});
