import { expect, test } from "vitest";
import { Lane } from "./lane.ts";

const settle = () => new Promise((done) => setTimeout(done, 0));

/** A call that answers when the test says. */
function held<T>(value: T) {
  let answer!: () => void;
  const sent = new Promise<T>((done) => (answer = () => done(value)));
  return { sent, answer };
}

test("at most cap calls are in flight; the rest go in order as earlier ones settle", async () => {
  const lane = new Lane(2);
  const sent: string[] = [];
  const calls = ["a", "b", "c", "d"].map((name) => ({ name, ...held(name) }));
  const answers = calls.map((c) =>
    lane.run(() => {
      sent.push(c.name);
      return c.sent;
    }),
  );
  await settle();
  expect(sent).toEqual(["a", "b"]);
  expect([lane.running, lane.waiting]).toEqual([2, 2]);
  calls[1]!.answer();
  await settle();
  expect(sent).toEqual(["a", "b", "c"]);
  calls[0]!.answer();
  calls[2]!.answer();
  calls[3]!.answer();
  await expect(Promise.all(answers)).resolves.toEqual(["a", "b", "c", "d"]);
  expect([lane.running, lane.waiting]).toEqual([0, 0]);
});

test("a call that throws or rejects settles as it did and frees its place", async () => {
  const lane = new Lane(1);
  await expect(
    lane.run(() => {
      throw new Error("no ipc");
    }),
  ).rejects.toThrow("no ipc");
  await expect(lane.run(() => Promise.reject(new Error("refused")))).rejects.toThrow("refused");
  await expect(lane.run(() => Promise.resolve(3))).resolves.toBe(3);
  expect(lane.running).toBe(0);
});
