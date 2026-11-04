// U16b's review states (registry-u16b.tsx): the room over a host with snoozed and done conversations, a matter snoozed
// whole and another waiting, then the lawyer's acts taken in order; and the snooze menu alone at a given hour.
import { useEffect, useRef, useState } from "react";
import { SnoozeItems } from "@clauseo/components/clauseo/snooze-menu.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { Menu, MenuPopup, MenuTrigger } from "@clauseo/components/ui/menu.tsx";
import type { MatterSummary } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "../room/fake-next.ts";
import type { ThreadInfo } from "../room/host-next.ts";
import { snoozeChoices } from "../room/snooze-choices.ts";
import { localZone } from "../room/when.ts";
import { RoomOver } from "./states.tsx";

const SHARMA: MatterSummary = {
  matter: uuid(1, 6),
  name: "Sharma v. State of Delhi",
  kind: "folder",
};
const JOSHI: MatterSummary = { matter: uuid(2, 6), name: "Joshi v. Joshi", kind: "folder" };
const KAPOOR: MatterSummary = {
  matter: uuid(3, 6),
  name: "Kapoor Exports v. State Bank of India",
  kind: "folder",
};
const RAO: MatterSummary = { matter: uuid(4, 6), name: "Rao v. Union of India", kind: "folder" };
const SCRATCH: MatterSummary = { matter: uuid(9, 6), name: "Scratch", kind: "scratch" };

/** Today at h:00 on this computer's clock, `days` on. */
const at = (h: number, days = 0) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days, h).getTime();
};
/** An ISO day `days` on. */
const isoIn = (days: number) => {
  const d = new Date(at(12, days));
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

function lists(): Map<string, ThreadInfo[]> {
  const now = Date.now();
  const of = (
    n: number,
    m: MatterSummary,
    title: string,
    over: Partial<ThreadInfo> = {},
  ): ThreadInfo => ({
    thread: uuid(n, 7),
    matter: m.matter,
    title,
    agent: "claude-acp",
    status: "idle",
    seen: true,
    last_activity: now - n * 600_000,
    ...over,
  });
  return new Map([
    [
      SHARMA.matter,
      [
        // the newest id opens
        of(20, SHARMA, "Bail conditions for Mr. Sharma"),
        of(2, SHARMA, "List of dates", { status: "needs_you" }),
        of(3, SHARMA, "Quashing petition, first draft", {
          seen: false,
          last_stop: { reason: "end_turn", at: now - 1_800_000 },
        }),
        of(4, SHARMA, "Witness statements", { snoozed_until: at(9, 1) }),
        of(5, SHARMA, "Chronology from the FIR", { done: true }),
        of(6, SHARMA, "Earlier bail order, 2024", { done: true }),
        of(7, SHARMA, "Section 482 or 528, which applies", { done: true }),
      ],
    ],
    [JOSHI.matter, [of(8, JOSHI, "Maintenance arrears", { snoozed_until: at(9, 4) })]],
    [
      KAPOOR.matter,
      [
        of(9, KAPOOR, "Reply to the notice", { snoozed_until: at(9, 1) }),
        of(10, KAPOOR, "Statement of account"),
      ],
    ],
    [RAO.matter, [of(11, RAO, "Draft rejoinder", { status: "needs_you" })]],
    [SCRATCH.matter, []],
  ]);
}

const triageHost = (): FakeTransport => {
  const threads = lists();
  return new FakeTransport({
    next: fakeNext({
      matters: () => [
        detailed(SHARMA, { last_opened: Date.now(), next_date: isoIn(8) }),
        detailed(JOSHI, { next_date: isoIn(3) }),
        detailed(KAPOOR),
        detailed(RAO, { status: "needs_permission" }),
        detailed(SCRATCH),
      ],
      threads: (m) => threads.get(m) ?? [],
      frames: true,
    }),
  });
};

export type Act =
  | { arm: string }
  | { press: string }
  | { item: string }
  | { focus: string }
  | { hover: string };

/** The room, then `acts` taken as the lawyer would, each once the last has settled: the pointer coming to the
 * conversation titled `arm`, a press on `press` (a selector), the menu row reading `item`, the focus on `focus`, or the
 * pointer resting on `hover` (its tooltip open). */
export function RoomTriage({ acts = [] }: { acts?: readonly Act[] }) {
  const taken = useRef(false);
  useEffect(() => {
    // once, though Strict Mode runs the effect twice
    if (taken.current) return;
    taken.current = true;
    const settle = () => new Promise((resolve) => setTimeout(resolve, 150));
    // what an act names may come with the later chunk: waited for, up to 5 s
    const found = async (selector: string) => {
      for (let tries = 0; tries < 50; tries += 1) {
        const el = document.querySelector<HTMLElement>(selector);
        if (el !== null) return el;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      return null;
    };
    void (async () => {
      await settle();
      for (const act of acts) {
        if ("arm" in act)
          [...document.querySelectorAll(".room-conv-item")]
            .find((li) => li.querySelector(".room-conv-title")?.textContent === act.arm)
            ?.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
        else if ("press" in act) (await found(act.press))?.click();
        else if ("focus" in act) (await found(act.focus))?.focus();
        else if ("hover" in act) {
          // the pointer comes to it and rests there: its tooltip opens after the page's delay
          const el = await found(act.hover);
          const box = el?.getBoundingClientRect();
          const at = { clientX: (box?.x ?? 0) + 4, clientY: (box?.y ?? 0) + 4 };
          for (const type of ["pointerover", "pointerenter", "pointermove"])
            el?.dispatchEvent(
              new PointerEvent(type, {
                ...at,
                pointerType: "mouse",
                bubbles: type !== "pointerenter",
              }),
            );
          for (const type of ["mouseover", "mouseenter", "mousemove"])
            el?.dispatchEvent(new MouseEvent(type, { ...at, bubbles: type !== "mouseenter" }));
          await new Promise((resolve) => setTimeout(resolve, 900));
        } else {
          for (let tries = 0; tries < 50; tries += 1) {
            const row = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((i) =>
              i.textContent?.includes(act.item),
            );
            if (row !== undefined) {
              row.click();
              break;
            }
            await new Promise((resolve) => setTimeout(resolve, 100));
          }
        }
        await settle();
      }
    })();
  }, [acts]);
  return <RoomOver host={triageHost} />;
}

/** The snooze menu alone, its times worked out for `hour`:00 today, with the open matter's next date `nextIn` days on, or
 * none. */
export function MenuAt({ hour, nextIn }: { hour: number; nextIn?: number }) {
  const next = nextIn === undefined ? undefined : isoIn(nextIn);
  const { lang } = useDisplay();
  const [now] = useState(() => at(hour));
  return (
    <div className="min-h-dvh p-s5">
      <Menu open modal={false}>
        <MenuTrigger render={<span />} />
        <MenuPopup>
          <SnoozeItems
            choices={snoozeChoices(now, localZone(), next, lang, () => {})}
            onChoose={() => {}}
          />
        </MenuPopup>
      </Menu>
    </div>
  );
}
