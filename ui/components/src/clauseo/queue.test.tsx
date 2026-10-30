// @vitest-environment happy-dom
// Messages while Clauseo works, rendered with invented messages: the queue over the composer, its full and held
// lines, editing and removing a message, the line that steers the running answer, and the wait for another
// conversation in the same matter.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, test } from "vitest";
import { mt } from "../lib/words-matters.ts";
import { OtherConversationWait, QueueStrip, SteerHint, SteerMark, SteerNote } from "./queue.tsx";
import { QUEUE_CAP, type QueuedMessage } from "./queue-model.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
});

const txt = (n: Element | null | undefined) => (n?.textContent ?? "").replace(/\u00a0/g, " ");
const press = async (label: string) => {
  const b = [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (x) => txt(x).trim() === label,
  );
  if (b === undefined) throw new Error(`no button ${label}`);
  await act(async () => b.click());
};

const messages = (n: number): QueuedMessage[] =>
  Array.from({ length: n }, (_, i) => ({
    id: `q${i + 1}`,
    text: `Check annexure ${i + 1} against the notice.`,
  }));
const none = () => {};

describe("the queue over the composer", () => {
  test("two slips show, then a row that opens the rest; the head is one line", async () => {
    await mount(
      <QueueStrip
        items={messages(3)}
        onEdit={none}
        onRemove={none}
        onSendNow={none}
        onSendNext={none}
      />,
    );
    expect(document.querySelectorAll(".queue-item")).toHaveLength(2);
    expect(txt(document.querySelector(".queue-say"))).toBe("3 queued, sent in order.");
    await press("1 more");
    expect(document.querySelectorAll(".queue-item")).toHaveLength(3);
    // Edit and Remove come in under the pointer; Send now stays
    expect(document.querySelectorAll(".queue-item .queue-more-act")).toHaveLength(6);
    expect(
      document.querySelector(".queue-item .queue-send")?.classList.contains("queue-more-act"),
    ).toBe(false);
    await press("Show fewer");
    expect(document.querySelectorAll(".queue-item")).toHaveLength(2);
  });

  test("one message says it goes once the answer is done; an empty queue shows nothing", async () => {
    await mount(
      <QueueStrip
        items={messages(1)}
        onEdit={none}
        onRemove={none}
        onSendNow={none}
        onSendNext={none}
      />,
    );
    expect(txt(document.querySelector(".queue-say"))).toBe(
      "1 queued, sent once this answer is done.",
    );
    expect(document.querySelector(".queue-fold")).toBeNull();
    await act(async () =>
      root?.render(
        <QueueStrip items={[]} onEdit={none} onRemove={none} onSendNow={none} onSendNext={none} />,
      ),
    );
    expect(document.querySelector(".queue")).toBeNull();
  });

  test("a full queue says so on its one line", async () => {
    await mount(
      <QueueStrip
        items={messages(QUEUE_CAP)}
        onEdit={none}
        onRemove={none}
        onSendNow={none}
        onSendNext={none}
      />,
    );
    expect(txt(document.querySelector(".queue-say"))).toBe(
      "10 queued, sent in order. The queue holds 10.",
    );
    expect(txt(document.querySelector(".queue-fold"))).toBe("8 more");
  });

  test("held after Stop, the queue says why, and Send next sends", async () => {
    let next = 0;
    await mount(
      <QueueStrip
        items={messages(2)}
        held={{ kind: "stopped", at: "4:32 pm" }}
        onEdit={none}
        onRemove={none}
        onSendNow={none}
        onSendNext={() => (next += 1)}
      />,
    );
    expect(txt(document.querySelector(".queue-say"))).toBe(
      "The queue is held. You stopped the answer at 4:32 pm, so nothing more is sent until you send.",
    );
    expect(document.querySelector(".queue")?.hasAttribute("data-held")).toBe(true);
    await press("Send next");
    expect(next).toBe(1);
  });

  test("Edit saves the new words, an emptied edit can't be saved, and Remove and Send now name their message", async () => {
    const edits: [string, string][] = [];
    const removed: string[] = [];
    const sent: string[] = [];
    await mount(
      <QueueStrip
        items={messages(2)}
        onEdit={(id, text) => edits.push([id, text])}
        onRemove={(id) => removed.push(id)}
        onSendNow={(id) => sent.push(id)}
        onSendNext={none}
      />,
    );
    const first = document.querySelector(".queue-item");
    await act(async () => first?.querySelectorAll<HTMLButtonElement>("button")[2]?.click());
    await act(async () =>
      document
        .querySelectorAll<HTMLButtonElement>(".queue-item")[1]
        ?.querySelectorAll("button")[1]
        ?.click(),
    );
    expect(sent).toEqual(["q1"]);
    expect(removed).toEqual(["q2"]);
    await act(async () =>
      document.querySelector<HTMLButtonElement>(".queue-item .queue-more-act")?.click(),
    );
    const field = document.querySelector<HTMLTextAreaElement>(".queue-edit");
    expect(document.querySelector("label[for='queue-edit-q1']")?.textContent).toBe(
      "Edit queued message 1",
    );
    const setValue = (v: string) =>
      field !== null && Reflect.set(HTMLTextAreaElement.prototype, "value", v, field);
    await act(async () => {
      setValue("   ");
      field?.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(
      [...document.querySelectorAll<HTMLButtonElement>("button")].find(
        (b) => b.textContent === "Save",
      )?.disabled,
    ).toBe(true);
    await act(async () => {
      setValue(" Check the annexures. ");
      field?.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await press("Save");
    expect(edits).toEqual([["q1", "Check the annexures."]]);
    expect(document.querySelector(".queue-edit")).toBeNull();
  });
});

describe("steering the running answer", () => {
  test("Send now and Queue are quiet buttons on the line above the field, and Enter queues", async () => {
    await mount(
      <SteerHint
        text={mt("en", "Send now gives your words to Clauseo while it writes.")}
        onSendNow={none}
        onQueue={none}
        full
      />,
    );
    expect(txt(document.querySelector(".steer-hint-text"))).toBe(
      "Send now gives your words to Clauseo while it writes.",
    );
    const queue = document.querySelector<HTMLButtonElement>(".steer-hint .mat-queue");
    expect(queue?.getAttribute("aria-keyshortcuts")).toBe("Enter");
    expect(queue?.className).not.toContain("bg-primary");
    // a full queue takes no more
    expect(queue?.disabled).toBe(true);
  });

  test("the lawyer's words stand in the answer, and a search moved aside is explained", async () => {
    await mount(
      <>
        <SteerMark
          who={mt("en", "You, {time}, while Clauseo answered", { time: "4:35 pm" })}
          text="Deal with the notice first."
        />
        <SteerNote
          text={mt(
            "en",
            "Your words at {time} came in while Clauseo searched Indian Kanoon for {what}. That search carried on in the background, finished at {done}, and this answer uses it.",
            {
              time: "4:35 pm",
              what: "judgments on limitation",
              done: "4:36 pm",
            },
          )}
          found={[{ at: "4:36 pm", what: "Acme Ltd v. State, (2001) 1 ABC 1" }]}
        />
      </>,
    );
    expect(txt(document.querySelector(".steer-mark"))).toContain("Deal with the notice first.");
    expect(txt(document.querySelector(".steer-note"))).toContain(
      "That search carried on in the background",
    );
  });
});

test("another conversation's wait says until when, and offers Queue or Stop the other", async () => {
  const pressed: string[] = [];
  await mount(
    <OtherConversationWait
      title="Draft plaint"
      by="4:41 pm"
      onQueue={() => pressed.push("queue")}
      onStopOther={() => pressed.push("stop")}
    />,
  );
  expect(txt(document.querySelector(".queue-wait-head"))).toBe(
    "Waiting for 'Draft plaint' to finish, by 4:41 pm",
  );
  await press("Stop the other");
  await press("Queue");
  expect(pressed).toEqual(["stop", "queue"]);
});
