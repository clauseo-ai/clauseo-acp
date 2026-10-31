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
import type { ViewEntry } from "../transport/protocol.ts";
import {
  applyBatch,
  entryId,
  entryIdOf,
  newView,
  positionOf,
  streamOf,
  textKey,
  type LiveView,
} from "./view.ts";

const AGENT = entryId("agent_text", uuid(1, 6));
const USER = entryId("user_message", uuid(1, 5));
const TOOL = entryId("tool_call", uuid(1, 7));

function started(): LiveView {
  const view = newView(STREAM);
  applyBatch(view, batch(1, [reset([userMessage(5, 1, "q"), agentText(6, 1, "ab")])]));
  return view;
}

describe("applyBatch", () => {
  test("a Reset replaces the view, header and texts included", () => {
    const v = started();
    expect(positionOf(v)).toEqual({ epoch: EPOCH, cursor: 1 });
    expect(v.header).toEqual({ header: header().header });
    expect(v.ids).toEqual([USER, AGENT]);
    expect(v.texts.get(textKey(AGENT))).toBe("ab");
    expect(v.texts.get(textKey(USER, 0))).toBe("q");
    expect(applyBatch(v, batch(2, [reset([toolCall(9, 1, "pending")])])).changed).toBe(true);
    expect(v.ids).toEqual([TOOL]);
    expect([...v.texts.keys()]).toEqual([]);
  });

  test("a replayed cursor is a no-op", () => {
    const v = started();
    const ids = v.ids;
    expect(applyBatch(v, batch(1, [append(1, 0, "zz")]))).toEqual({
      changed: false,
      resubscribe: false,
    });
    expect(v.texts.get(textKey(AGENT))).toBe("ab");
    expect(v.ids).toBe(ids);
  });

  test("AppendText writes only its text: the entry object and the id list keep their identity", () => {
    const v = started();
    const entry = v.entries.get(AGENT);
    const ids = v.ids;
    expect(applyBatch(v, batch(2, [append(1, 2, "cd"), append(1, 4, "e")])).changed).toBe(true);
    expect(v.texts.get(textKey(AGENT))).toBe("abcde");
    expect(v.entries.get(AGENT)).toBe(entry);
    expect(v.ids).toBe(ids);
    expect(v.cursor).toBe(3);
  });

  test("a from_len that is not the text's length asks to resubscribe after the last applied cursor", () => {
    const v = started();
    const r = applyBatch(v, batch(2, [append(1, 2, "c"), append(1, 9, "x")]));
    expect(r).toEqual({ changed: true, resubscribe: true });
    expect(positionOf(v)).toEqual({ epoch: EPOCH, cursor: 2 });
    expect(v.texts.get(textKey(AGENT))).toBe("abc");
  });

  test("a gap in the cursors and a new epoch without its Reset both ask to resubscribe, changing nothing", () => {
    const v = started();
    expect(applyBatch(v, batch(3, [upsert(toolCall(9, 1, "pending"))]))).toEqual({
      changed: false,
      resubscribe: true,
    });
    expect(
      applyBatch(v, batch(2, [upsert(toolCall(9, 1, "pending"))], uuid(9, 3))).resubscribe,
    ).toBe(true);
    expect(v.ids).toEqual([USER, AGENT]);
    expect(applyBatch(newView(STREAM), batch(1, [append(1, 0, "a")])).resubscribe).toBe(true);
  });

  test("upserts keep entries sorted by order; an update keeps the id list; remove drops the entry and its texts", () => {
    const v = started();
    applyBatch(v, batch(2, [upsert(toolCall(5.5, 1, "pending"))]));
    expect(v.ids).toEqual([USER, TOOL, AGENT]);
    const ids = v.ids;
    applyBatch(v, batch(3, [upsert(toolCall(5.5, 1, "completed"))]));
    expect(v.ids).toBe(ids);
    expect(v.entries.get(TOOL)?.tool_call?.status).toBe("completed");
    applyBatch(v, batch(4, [{ remove: { entry_id: AGENT } }, { remove: { entry_id: USER } }]));
    expect(v.ids).toEqual([TOOL]);
    expect([...v.texts.keys()]).toEqual([]);
  });

  test("a batch that adds several rows makes one new id list", () => {
    const v = started();
    const ids = v.ids;
    applyBatch(v, batch(2, [upsert(toolCall(7, 1, "pending")), upsert(toolCall(8, 2, "pending"))]));
    expect(v.ids).not.toBe(ids);
    expect(ids).toEqual([USER, AGENT]);
    expect(v.ids).toEqual([USER, AGENT, TOOL, entryId("tool_call", uuid(2, 7))]);
  });
});

test("entry ids and streams keep protocol's text forms", () => {
  expect(streamOf("t1")).toBe("thread:t1");
  const entry: ViewEntry = toolCall(1, 3, "pending");
  expect(entryIdOf(entry)).toBe(entryId("tool_call", uuid(3, 7)));
  expect(entryIdOf(entry)).toBe(`tool_call:${uuid(3, 7)}`);
});
