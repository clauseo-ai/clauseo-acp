import { describe, expect, test } from "vitest";
import {
  agentText,
  append,
  batch,
  EPOCH,
  header,
  reset,
  STREAM,
  toolCall,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import { applyBatch, emptyView, positionOf, textKey, type StreamView } from "./view.ts";

const AGENT = `agent_text:${uuid(1, 6)}`;

function started(): StreamView {
  const r = applyBatch(
    emptyView(STREAM),
    batch(1, [reset([userMessage(5, 1, "q"), agentText(6, 1, "ab")])]),
  );
  return r.view;
}

describe("applyBatch", () => {
  test("a Reset replaces the view, header and texts included", () => {
    const v = started();
    expect(positionOf(v)).toEqual({ epoch: EPOCH, cursor: 1 });
    expect(v.header).toEqual({ header: header().header });
    expect(v.ids).toEqual([`user_message:${uuid(1, 5)}`, AGENT]);
    expect(v.texts[textKey(AGENT)]).toBe("ab");
    expect(v.texts[textKey(`user_message:${uuid(1, 5)}`, 0)]).toBe("q");
    const again = applyBatch(v, batch(2, [reset([toolCall(9, 1, "pending")])])).view;
    expect(again.ids).toEqual([`tool_call:${uuid(1, 7)}`]);
    expect(again.texts).toEqual({});
  });

  test("a replayed cursor is a no-op that returns the same view", () => {
    const v = started();
    const r = applyBatch(v, batch(1, [append(1, 0, "zz")]));
    expect(r).toEqual({ view: v, advanced: false, resubscribe: false });
    expect(r.view).toBe(v);
  });

  test("AppendText changes only the text: the entry object and the id list keep their identity", () => {
    const v = started();
    const r = applyBatch(v, batch(2, [append(1, 2, "cd"), append(1, 4, "e")]));
    expect(r.advanced).toBe(true);
    expect(r.view.texts[textKey(AGENT)]).toBe("abcde");
    expect(r.view.entries.get(AGENT)).toBe(v.entries.get(AGENT));
    expect(r.view.ids).toBe(v.ids);
    expect(r.view.cursor).toBe(3);
    expect(v.texts[textKey(AGENT)]).toBe("ab");
  });

  test("a from_len that is not the text's length asks to resubscribe after the last applied cursor", () => {
    const v = started();
    const r = applyBatch(v, batch(2, [append(1, 2, "c"), append(1, 9, "x")]));
    expect(r.resubscribe).toBe(true);
    expect(positionOf(r.view)).toEqual({ epoch: EPOCH, cursor: 2 });
    expect(r.view.texts[textKey(AGENT)]).toBe("abc");
  });

  test("a gap in the cursors and a new epoch without its Reset both ask to resubscribe", () => {
    const v = started();
    expect(applyBatch(v, batch(3, [upsert(toolCall(9, 1, "pending"))])).resubscribe).toBe(true);
    const other = applyBatch(v, batch(2, [upsert(toolCall(9, 1, "pending"))], uuid(9, 3)));
    expect(other.resubscribe).toBe(true);
    expect(other.view).toBe(v);
    expect(applyBatch(emptyView(STREAM), batch(1, [append(1, 0, "a")])).resubscribe).toBe(true);
  });

  test("upserts keep entries sorted by order; remove drops the entry and its texts", () => {
    const v = started();
    const tool = `tool_call:${uuid(1, 7)}`;
    const a = applyBatch(v, batch(2, [upsert(toolCall(5.5, 1, "pending"))])).view;
    expect(a.ids).toEqual([`user_message:${uuid(1, 5)}`, tool, AGENT]);
    const updated = applyBatch(a, batch(3, [upsert(toolCall(5.5, 1, "completed"))])).view;
    expect(updated.ids).toBe(a.ids);
    expect(updated.entries.get(tool)?.tool_call?.status).toBe("completed");
    const removed = applyBatch(updated, batch(4, [{ remove: { entry_id: AGENT } }])).view;
    expect(removed.ids).toEqual([`user_message:${uuid(1, 5)}`, tool]);
    expect(removed.texts[textKey(AGENT)]).toBeUndefined();
  });
});
