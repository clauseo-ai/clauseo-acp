// @vitest-environment happy-dom
// R17 PR-E's notes: the full pool by the sandbox's why (none or unknown, no free place, two conversations already in
// the matter, which offers no pool action), the reset line and "Free up places" only while places need a reset, and
// an item a place didn't hand back (pool_owned), with no retry and its IT line naming where when known; M4's refusals
// (a broken set-up, a block, a cause from a newer Clauseo), with no retry.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { NotProtectedNote, type NotePool } from "./protection.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot> | null = null;
const tick = () => act(async () => new Promise<void>((r) => setTimeout(r, 60)));

async function show(node: React.ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
  await tick();
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

const byId = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const text = () => byId("np-note")?.textContent ?? "";
const POOL: NotePool = { size: 8, open: 5, held: 0, owed: 2, set_aside: 1 };

const FALLBACK =
  "All 8 places in this computer's Safe workspace are in use, so Clauseo can't answer in this matter until one is free. End a conversation in another matter to free one.";
const NO_FREE =
  "All 8 places in this computer's Safe workspace are in use by open conversations. End a conversation in another matter to free one.";
const MATTER_FULL =
  "Two conversations are already working in this matter. Try again when one finishes.";

const pool = (over: Partial<NotePool>, addRoom = true, onFreeUp?: () => void) =>
  show(
    <NotProtectedNote
      open
      matter="Mehta v. Rao"
      why="pool"
      pool={{ ...POOL, ...over }}
      addRoom={addRoom}
      onClose={() => {}}
      onProtect={() => {}}
      onFreeUp={onFreeUp}
    />,
  );

test.each([
  [undefined, FALLBACK],
  [null, FALLBACK],
  ["unknown", FALLBACK],
  ["a_why_from_later", FALLBACK],
  ["no_free_slot", NO_FREE],
  ["matter_sessions_full", MATTER_FULL],
])("a full pool with why %s says its own body", async (why, body) => {
  await pool({ why });
  expect(text()).toContain(body);
  expect(text()).not.toContain("works without it");
});

test("the plain and no-free-place pools keep their counts and Add room; no line repeats ending a conversation", async () => {
  await pool({ why: "no_free_slot" });
  expect(byId("np-pool")).not.toBeNull();
  expect(byId("protection-setup")?.textContent).toBe("Add room");
  expect(text()).not.toContain("To free one now");
  expect(byId("np-pool-reset")).toBeNull();
  expect(byId("protection-free-up")).toBeNull();
});

test("two conversations already in the matter: no counts, no Add room, no reset, no Free up places", async () => {
  const onFreeUp = vi.fn();
  await pool({ why: "matter_sessions_full", needs_reset: 2 }, true, onFreeUp);
  expect(byId("np-pool")).toBeNull();
  expect(byId("np-pool-out")).toBeNull();
  expect(byId("protection-setup")).toBeNull();
  expect(byId("np-pool-reset")).toBeNull();
  expect(byId("protection-free-up")).toBeNull();
});

test("the reset line and Free up places show only while places need a reset", async () => {
  const onFreeUp = vi.fn();
  await pool({ why: "no_free_slot", needs_reset: 0 }, true, onFreeUp);
  expect(byId("np-pool-reset")).toBeNull();
  expect(byId("protection-free-up")).toBeNull();
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await pool({ why: "no_free_slot", needs_reset: 1 }, true, onFreeUp);
  expect(byId("np-pool-reset")?.textContent).toBe(
    "One of them needs resetting before it can be used again. Windows asks for permission first.",
  );
  await act(async () => byId("protection-free-up")?.click());
  expect(onFreeUp).toHaveBeenCalledOnce();
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await pool({ needs_reset: 3 }, false, onFreeUp);
  expect(byId("np-pool-reset")?.textContent).toBe(
    "3 of them need resetting before they can be used again. Windows asks for permission first.",
  );
  expect(byId("protection-free-up")?.textContent).toBe("Free up places");
  // at the largest size: no Add room, Free up places still
  expect(byId("protection-setup")).toBeNull();
});

test("pool_owned: its body, the IT line with where or with it, and no retry", async () => {
  await show(
    <NotProtectedNote
      open
      matter="Mehta v. Rao"
      why="pool_owned"
      onClose={() => {}}
      onProtect={() => {}}
    />,
  );
  expect(byId("np-note")?.dataset["cause"]).toBe("pool_owned");
  expect(text()).toContain(
    "Part of this matter's folder wasn't handed back to you after the Safe workspace last used it, so Clauseo can't answer in this matter until it is.",
  );
  expect(text()).toContain("Ask your IT team to give you back ownership of it.");
  expect(byId("protection-setup")).toBeNull();
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await show(
    <NotProtectedNote
      open
      matter="Mehta v. Rao"
      why="pool_owned"
      where="Pleadings/Draft.docx"
      onClose={() => {}}
    />,
  );
  expect(text()).toContain(
    "Ask your IT team to give you back ownership of \u2068Pleadings/Draft.docx\u2069.",
  );
});

test.each([
  [
    "set_up_failed",
    "The Safe workspace on this computer isn't working, so Clauseo can't answer here until it's set up again.",
  ],
  ["blocked", "Something on this computer stops the Safe workspace, so Clauseo can't answer here."],
  [
    "unknown",
    "This version of Clauseo doesn't recognise what's stopping the Safe workspace, so it can't answer here. Update Clauseo, or ask your IT team.",
  ],
] as const)(
  "M4, %s: its refusal, never the works-without-it line, and no retry",
  async (why, line) => {
    await show(
      <NotProtectedNote
        open
        matter="Mehta v. Rao"
        why={why}
        onClose={() => {}}
        onProtect={() => {}}
      />,
    );
    expect(byId("np-note")?.dataset["cause"]).toBe(why);
    expect(text()).toContain(line);
    expect(text()).not.toContain("can read any file");
    expect(byId("protection-setup")).toBeNull();
  },
);

test("#722's probe_failed: its line, never the works-without-it line, and Try again", async () => {
  let pressed = 0;
  await show(
    <NotProtectedNote
      open
      matter="Mehta v. Rao"
      why="probe_failed"
      onClose={() => {}}
      onProtect={() => (pressed += 1)}
    />,
  );
  expect(byId("np-note")?.dataset["cause"]).toBe("probe_failed");
  expect(text()).toContain(
    "Clauseo couldn't check the Safe workspace on this computer, so it can't answer here yet. Try again. If it keeps happening, restart Clauseo.",
  );
  expect(text()).not.toContain("can read any file");
  expect(byId("protection-setup")?.textContent).toBe("Try again");
  await act(async () => byId("protection-setup")!.click());
  expect(pressed).toBe(1);
});
