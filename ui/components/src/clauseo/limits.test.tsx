// @vitest-environment happy-dom
// The usage limit's parts and the plan's card, rendered with invented words: where an answer stopped, the note's
// rows and its yellow only while a choice waits, the quiet note under the field, the day's line, and the plan's
// windows, with no empty bar where there are no figures.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, test } from "vitest";
import {
  DayLine,
  DraftExcerpt,
  FieldNotice,
  NoticeCard,
  RoomBand,
  WayTile,
  WordCard,
} from "./limit-note.tsx";
import { PlanCard, type PlanCardProps } from "./plan-card.tsx";

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

describe("the limit's note", () => {
  test("the paragraph stops where the answer left it, with the mark on the last one only", async () => {
    await mount(
      <DraftExcerpt
        file="Plaint.docx"
        paras={["The plaintiff supplied steel.", "The defendant did not pay, and the"]}
        stop="Stopped here at 4:52 pm"
      />,
    );
    const paras = document.querySelectorAll(".lim-draft-para");
    expect(paras).toHaveLength(2);
    expect(paras[0]?.querySelector(".lim-stop")).toBeNull();
    expect(txt(paras[1])).toContain("The defendant did not pay, and the");
    expect(txt(document.querySelector(".lim-stop"))).toBe("Stopped here at 4:52 pm");
  });

  test("yellow sits on what waits for the lawyer's hand, not on the limit", async () => {
    await mount(
      <>
        <NoticeCard
          className="lim-note-limit"
          icon={null}
          label="Plan limit"
          title="Claude plan limit reached at 4:52 pm"
          rows={[
            {
              label: "Finished",
              text: (
                <WordCard title="Plaint.docx" sub="12 tracked changes" state="Waiting for you" />
              ),
            },
            { label: "Left", text: "Paragraphs 13 to 20" },
          ]}
          actions={<button type="button">Continue at 6:10 pm</button>}
          note="At 6:10 pm, when the plan resets, Clauseo starts a new request."
          other={{
            text: "The filing won't wait for the reset.",
            action: <button type="button">Ask ChatGPT instead</button>,
          }}
        />
        <NoticeCard waiting icon={null} label="Choice" title="Which date should the plaint use?" />
      </>,
    );
    const [limit, choice] = document.querySelectorAll(".lim-note");
    expect(limit?.hasAttribute("data-waiting")).toBe(false);
    expect(choice?.hasAttribute("data-waiting")).toBe(true);
    expect(
      [...(limit?.querySelectorAll(".lim-note-label") ?? [])].map((n) => n.textContent),
    ).toEqual(["Finished", "Left"]);
    expect(txt(limit?.querySelector(".lim-word"))).toContain("Waiting for you");
    // the line follows the actions unless it's a confirmation
    expect(limit?.querySelector(".lim-note-go")?.lastElementChild?.className).toBe("lim-note-line");
    expect(txt(limit?.querySelector(".lim-note-other"))).toContain("Ask ChatGPT instead");
    // a note with nothing to do draws no action row
    expect(choice?.querySelector(".lim-note-go")).toBeNull();
  });

  test("advice under the field carries no clock; a wait does", async () => {
    await mount(<FieldNotice lead="Your Claude plan is running low." rest="Ask in two halves." />);
    expect(txt(document.querySelector(".lim-field-lead"))).toBe("Your Claude plan is running low.");
    expect(txt(document.querySelector(".lim-field-rest"))).toBe("Ask in two halves.");
    expect(document.querySelector(".lim-field-note .lim-field-icon")).toBeNull();
    await act(async () =>
      root?.render(<FieldNotice wait lead="Waiting for the reset at 6:10 pm." />),
    );
    expect(document.querySelector(".lim-field-note .lim-field-icon")).not.toBeNull();
    expect(document.querySelector(".lim-field-rest")).toBeNull();
  });

  test("a day's line, a way on, and the band across the work say what they're given", async () => {
    await mount(
      <>
        <DayLine>Today</DayLine>
        <WayTile
          title="A Claude API key"
          account=""
          body="Billed by use."
          action={<button type="button">Add a key</button>}
        />
        <RoomBand title="Offline" rows={[{ label: "Still works", text: "Reading papers" }]} />
      </>,
    );
    expect(txt(document.querySelector(".lim-day"))).toBe("Today");
    expect(document.querySelector(".lim-way-account")).toBeNull();
    expect(txt(document.querySelector(".lim-band"))).toContain("Reading papers");
  });
});

describe("the plan's card", () => {
  const card: PlanCardProps = {
    plan: "Claude plan",
    updated: "As of 4:28 pm",
    windows: [
      {
        name: "5-hour limit",
        left: 18,
        leftText: "18% left",
        reset: "Resets 6:10 pm",
        valueText: "18% left",
      },
      {
        name: "Weekly limit",
        left: 30,
        leftText: "30% left",
        reset: "Resets Tuesday 9:00 am",
        valueText: "30% left",
      },
    ],
    foot: "At the limit, Claude stops where it is.",
    manage: { label: "Manage plan", href: "https://example.com/plan" },
  };

  test("every window, what's left with its bar, when it was updated, and Manage plan", async () => {
    await mount(<PlanCard {...card} />);
    const body = document.querySelector(".plan-card-body");
    expect(txt(body)).toContain("5-hour limit");
    expect(txt(body)).toContain("18% left");
    expect(body?.querySelector('[role="meter"]')?.getAttribute("aria-valuenow")).toBe("18");
    expect(txt(body)).toContain("Weekly limit");
    expect(txt(body)).toContain("As of 4:28 pm");
    expect(txt(body?.querySelector(".plan-card-manage"))).toBe("Manage plan");
    // the card adds no second banner
    expect(body?.querySelector("header, footer")).toBeNull();
  });

  test("missing figures draw no empty bar, and billed by use has no limit line", async () => {
    await mount(
      <PlanCard
        {...card}
        windows={[
          { name: "5-hour limit", left: null, leftText: "", reset: "", valueText: "No figures" },
        ]}
        foot={null}
        stale
      />,
    );
    expect(document.querySelector(".plan-track")).toBeNull();
    expect(document.querySelector(".plan-window-reset")).toBeNull();
    expect(txt(document.querySelector(".plan-card-foot"))).not.toContain("At the limit");
    expect(
      document.querySelector(".plan-card-updated")?.classList.contains("plan-card-stale"),
    ).toBe(true);
  });
});
