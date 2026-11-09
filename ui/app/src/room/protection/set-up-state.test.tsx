// @vitest-environment happy-dom
// R17 PR-D3a in the room: a build that sets the Safe workspace up but doesn't start Claude or
// ChatGPT in it yet sends `set_up_state` beside a `computer` of `unsupported { not_built }`. Only
// Settings reads it: the composer's line, the toolbar's and the rail's marks, the Not protected
// note, the firm's composer notice and the first run's step say exactly what they say without it,
// for a set-up state of protected and for each of its causes. Linux's `unsupported { os }` reads
// as a computer that can't run the Safe workspace.
import { tbt } from "@clauseo/components/lib/words-toolbar.ts";
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { MatterSummary, Protection, ProtectionView } from "../../transport/protocol.ts";
import { mountRoom, summary, THE_MATTER, type Harness } from "../thread/harness.ts";
import type { ProtectionState } from "../../store/protection.ts";
import { workspaceStep } from "../first-run/workspace.ts";
import { asksAdmin, computerCause, inProgress } from "./view.ts";
import { blockOf, needsInstaller } from "./blocked.ts";
import { np, PROT, view } from "./fixtures.ts";

let h: Harness | null = null;
beforeEach(() => localStorage.clear());
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const NOT_BUILT = np({ unsupported: { why: "not_built" } });

const STATES = [
  ["protected (tier b)", { protected: { tier: "b" } } as Protection],
  ["needs_set_up", PROT.needs],
  ["set_up_refused", PROT.refused],
  ["set_up_failed", PROT.failed],
  ["blocked (logon_right)", PROT.logon],
  ["blocked (policy)", PROT.policy],
  ["blocked (admin_copy_missing)", PROT.missing],
  ["other_account", PROT.otherAccount],
] as const;

const settle = async () => {
  for (let i = 0; i < 5; i += 1) await h!.settle();
};

/** The room over a not-built computer, with `setUp` as its set-up state (none: absent), and what it shows. */
async function seen(
  setUp: Protection | undefined,
  over: Partial<ProtectionView> = {},
  agent = "claude-acp",
  computer: Protection = NOT_BUILT,
) {
  const m: MatterSummary = { ...THE_MATTER, protection: computer };
  h = await mountRoom({
    matters: [m],
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    threads: [{ ...summary(), agent }],
    protection: {
      view: () =>
        view(computer, {
          ...(setUp === undefined ? {} : { set_up_state: setUp }),
          ...over,
        } as Partial<ProtectionView>),
    },
  });
  await settle();
  const line = h.one("workspace-line");
  const shown = {
    line: line?.textContent ?? null,
    cause: line?.dataset["cause"] ?? null,
    toolbar: h.host.querySelector(".room-next .np-mark")?.textContent ?? null,
    rail:
      h
        .all("matter-row")
        .find((r) => r.textContent?.includes(THE_MATTER.name))
        ?.querySelector(".room-matter-np") !== null,
    required: h.one("protection-required")?.textContent ?? null,
    note: "",
    noteCause: "" as string | undefined,
    setUpButton: false,
  };
  await h.click(line);
  await settle();
  const note = document.body.querySelector<HTMLElement>('[data-testid="np-note"]');
  shown.note = note?.textContent ?? "";
  shown.noteCause = note?.dataset["cause"];
  shown.setUpButton = document.body.querySelector('[data-testid="protection-setup"]') !== null;
  await h.unmount();
  h = null;
  document.body.replaceChildren();
  localStorage.clear();
  return shown;
}

test.each(STATES)(
  "set_up_state %s: the matter's line, badges, note and composer say exactly what not-built alone says",
  async (_, state) => {
    const before = await seen(undefined);
    expect(before.cause).toBe("unsupported");
    expect(before.toolbar).toBe(tbt("en", "Not protected"));
    expect(before.rail).toBe(true);
    expect(before.noteCause).toBe("not_built");
    expect(before.setUpButton).toBe(false);
    expect(await seen(state)).toEqual(before);
  },
);

test.each(STATES)(
  "set_up_state %s on ChatGPT, with auto modes not allowed: still not protected, in ChatGPT's words",
  async (_, state) => {
    const over = { unprotected_auto_allowed: false } as Partial<ProtectionView>;
    const before = await seen(undefined, over, "codex-acp");
    expect(before.line).toBe(
      `${tbt("en", "Not protected.")} ${tbt("en", "Clauseo can change files in this matter without asking. It asks before going beyond it.")}`,
    );
    expect(await seen(state, over, "codex-acp")).toEqual(before);
  },
);

test.each(STATES)(
  "set_up_state %s where the firm requires the Safe workspace: the composer's notice is not-built's",
  async (_, state) => {
    const over = { required: true } as Partial<ProtectionView>;
    const before = await seen(undefined, over);
    expect(before.required).toContain("comes in an update");
    expect(await seen(state, over)).toEqual(before);
  },
);

const READERS = { asksAdmin, computerCause, inProgress, blockOf, needsInstaller };
const stateOf = (v: ProtectionView, step: ProtectionState["step"]): ProtectionState =>
  ({ view: v, step, running: null, failed: null, managedBy: null }) as unknown as ProtectionState;

test.each(STATES)(
  "set_up_state %s: the first run has no Safe workspace step, as not-built alone",
  (_, state) => {
    const v = view(NOT_BUILT, { set_up_state: state } as Partial<ProtectionView>);
    expect(workspaceStep(stateOf(v, null), READERS)).toBeNull();
    expect(workspaceStep(stateOf(view(NOT_BUILT), null), READERS)).toBeNull();
  },
);

test("Linux's unsupported { os }: Not protected, and the note says this computer can't run the Safe workspace, with no set-up", async () => {
  const shown = await seen(undefined, {}, "claude-acp", np({ unsupported: { why: "os" } }));
  expect(shown.cause).toBe("unsupported");
  expect(shown.toolbar).toBe(tbt("en", "Not protected"));
  expect(shown.noteCause).toBe("cant");
  expect(shown.note).toContain(
    pw(
      "en",
      "This computer can't run the Safe workspace, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.",
    ),
  );
  expect(shown.note).not.toMatch(/\bagent\b|\bIT\b/);
  expect(shown.setUpButton).toBe(false);
  const v = view(np({ unsupported: { why: "os" } }));
  expect(workspaceStep(stateOf(v, null), READERS)).toBeNull();
});
