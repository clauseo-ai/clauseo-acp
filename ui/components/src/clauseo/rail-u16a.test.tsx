// @vitest-environment happy-dom
// U16a's rail parts, rendered: yellow only on a question, a permission and the limit's open choice, over every status's
// mark, matters' and conversations'; Failed's triangle in concrete with its words loud; a Needs-you row's conversation
// beneath its status; Matters / Recent and Recent's rows.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { IconFailed } from "../icons/icons.tsx";
import { iconIn } from "../icons/icon-in.tsx";
import { RailMark } from "./rail-mark.tsx";
import { CONVERSATION_MARK, MATTER_MARK } from "./rail-marks.ts";
import { MatterRow } from "./rail-matter.tsx";
import { RailRecent } from "./rail-recent.tsx";
import { RailSwitch } from "./rail-switch.tsx";
import { conversationStatus, matterStatus } from "./rail-words.ts";

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

test("yellow stands only on a question, a permission and the limit's open choice, over every status's mark", async () => {
  const yellow = new Set(["question", "permission", "paused"]);
  for (const [state, mark] of Object.entries(MATTER_MARK))
    expect(mark.waits, `matter ${state}`).toBe(yellow.has(state));
  for (const [state, mark] of Object.entries(CONVERSATION_MARK))
    expect(mark.waits, `conversation ${state}`).toBe(
      state === "question" || state === "permission",
    );
  // the drawn mark: the disc only where it waits
  await mount(
    <>
      {Object.entries(MATTER_MARK).map(([state, mark]) => (
        <span key={state} data-state={state}>
          <RailMark mark={mark} />
        </span>
      ))}
    </>,
  );
  const discs = [...(host?.querySelectorAll("[data-waits]") ?? [])].map(
    (m) => m.parentElement?.dataset["state"],
  );
  expect([...discs].sort((a, b) => (a ?? "").localeCompare(b ?? ""))).toEqual([
    "paused",
    "permission",
    "question",
  ]);
});

test("Failed takes the warning triangle and says when, today or on another day", () => {
  expect(MATTER_MARK.failed.icon).toBe(IconFailed);
  expect(CONVERSATION_MARK.failed.icon).toBe(IconFailed);
  expect(matterStatus("en", "failed", "3:12 pm")).toBe("Failed at 3:12\u00a0pm");
  expect(matterStatus("en", "failed", "3:12 pm", "Monday")).toBe(
    "Failed on Monday at 3:12\u00a0pm",
  );
  expect(conversationStatus("en", "failed", "3:12 pm")).toBe("Failed at 3:12\u00a0pm");
});

test("a Needs-you row: the name, the status loud with its mark, the conversation beneath it quiet", async () => {
  await mount(
    <ul>
      <MatterRow
        name="In re Gupta Textiles"
        note=""
        hint=""
        state="failed"
        at="3:12 pm"
        where="Later judgments"
      />
    </ul>,
  );
  const row = host?.querySelector(".room-matter-row");
  expect(row?.getAttribute("data-state")).toBe("failed");
  expect(row?.querySelector(".text-primary.font-chip")?.textContent?.replace(/\u00a0/g, " ")).toBe(
    "Failed at 3:12 pmLater judgments",
  );
  expect(row?.querySelector(".room-matter-where")?.textContent).toBe("Later judgments");
  expect(iconIn(row?.querySelector(".room-mark"), IconFailed, 14)).not.toBeNull();
});

test("Matters / Recent: two toggle buttons, the chosen one pressed", async () => {
  const chosen: string[] = [];
  await mount(<RailSwitch view="matters" onView={(v) => chosen.push(v)} testId="rail-switch" />);
  const [matters, recent] = [...(host?.querySelectorAll("button") ?? [])];
  expect(matters?.textContent).toBe("Matters");
  expect(recent?.textContent).toBe("Recent");
  expect(matters?.getAttribute("aria-pressed")).toBe("true");
  expect(recent?.getAttribute("aria-pressed")).toBe("false");
  await act(async () => recent?.click());
  expect(chosen).toEqual(["recent"]);
});

test("Recent's rows: the title, its mark at the line's end, the matter beneath; only a question says its words", async () => {
  await mount(
    <RailRecent
      days={[
        {
          day: "Today",
          rows: [
            {
              id: "1",
              title: "List of dates",
              matter: "Sharma v. State of Delhi",
              state: "question",
            },
            { id: "2", title: "Counterclaim", matter: "Mehta v. Kapoor", state: "ready" },
            {
              id: "3",
              title: "Bail conditions",
              matter: "Sharma v. State of Delhi",
              current: true,
            },
          ],
        },
        { day: "Yesterday", rows: [{ id: "4", title: "Cheque bounce", matter: "Scratch matter" }] },
      ]}
    />,
  );
  expect([...(host?.querySelectorAll(".room-when") ?? [])].map((h) => h.textContent)).toEqual([
    "Today",
    "Yesterday",
  ]);
  const rows = [...(host?.querySelectorAll<HTMLButtonElement>(".room-recent-row") ?? [])];
  expect(rows[0]?.querySelector(".room-conv-ask")?.textContent).toBe("A question for you");
  expect(rows[0]?.querySelector("[data-waits]")).not.toBeNull();
  expect(rows[1]?.querySelector(".room-conv-ask")).toBeNull();
  expect(rows[1]?.querySelector(".sr-only")?.textContent).toBe("Answer ready");
  expect(rows[1]?.querySelector(".room-matter-note")?.textContent).toBe("Mehta v. Kapoor");
  expect(rows[2]?.getAttribute("aria-current")).toBe("true");
  expect(rows[2]?.querySelector(".room-mark")).toBeNull();
});
