// @vitest-environment happy-dom
// UC's Safe workspace step in the first run, over a FakeTransport with the setup and protection wires: no step when the
// computer is protected or the protection query is refused, so the first read starts at once; the Windows screen for
// needs_set_up, which is itself the confirm; "Not now", a refusal and a failure each count as the step's outcome, the
// room's line then says Not protected with Try again, and UD2's first read starts after it, once; where R17 says the
// set-up asks no administrator, it is sent once on entering, with the wait.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { tbt } from "@clauseo/components/lib/words-toolbar.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { batch, event, header, reset, THREAD, TURN } from "../../testing/stream-script.ts";
import {
  FakeTransport,
  type FakeHost,
  type FakeProtectionHost,
} from "../../transport/fake-transport.ts";
import type {
  CommandOutcome,
  MatterSummary,
  PaperSummary,
  Protection,
  ProtectionView,
  ThreadSummary,
} from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import { forgetMarksForTests, MARKS_KEY } from "../first-read/marks.ts";
import { PROT, view } from "../protection/fixtures.ts";
import { Room } from "../room.tsx";
import { connectRoom } from "../start.ts";
import { stubFonts } from "../thread/testing.ts";
import {
  byId,
  CLAUDE,
  info,
  install,
  MADE_MATTER,
  play,
  press,
  READY,
  settle,
  signIn,
  SIGNED_IN,
} from "./rig.tsx";

stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  localStorage.clear();
  forgetMarksForTests();
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const PAPER = { path: "Plaint.pdf", file: "file-1", size: 1000 } as unknown as PaperSummary;

type Answer = { resolve: () => void; reject: (e: unknown) => void };

/** The room over the setup, papers and protection wires on Windows; the protection commands wait for the test. */
async function mount(protection: FakeProtectionHost | undefined) {
  let matters: MatterSummary[] = [];
  let made: ThreadSummary[] = [];
  const fake: FakeHost = {
    platform: "windows",
    query: async (query) => {
      if (query === "list_matters") return { matters };
      if (typeof query === "object" && "list_threads" in query) return { threads: made };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (envelope): CommandOutcome => {
      const c = envelope.command;
      if (c.create_thread !== undefined) {
        made = [
          {
            thread: THREAD,
            matter: c.create_thread.matter,
            agent: CLAUDE,
            status: "running",
            title: "First read",
          },
        ];
        return { thread_created: { thread: THREAD } };
      }
      if (c.send_prompt !== undefined) return { prompt_accepted: { turn: TURN, message: THREAD } };
      if (c.create_matter !== undefined) {
        matters = [{ matter: MADE_MATTER, name: c.create_matter.name, kind: "folder" }];
        return { matter_created: { matter: MADE_MATTER } };
      }
      return "done";
    },
    pickFolder: () => ({ folder: "folder-ref-1", display: "Documents/Matters/Rathi Steels" }),
    setup: { first: () => [info(), install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })] },
    next: fakeNext({
      matters: () => [],
      papers: [PAPER],
      agents: [{ id: CLAUDE, kind: "claude" }],
      parts: ["agents", "papers"],
    }),
    subscribe: (stream) =>
      queueMicrotask(() =>
        transport.deliver(event(batch(1, [reset([], header("running"))], undefined, stream))),
      ),
    ...(protection === undefined ? {} : { protection }),
  };
  const transport: FakeTransport = new FakeTransport(fake);
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  await press(byId("open-folder"));
  await settle();
  return transport;
}

const reads = (t: FakeTransport) =>
  t.only("dispatch").filter((d) => d.command.command.create_thread !== undefined).length;
const step = () => byId("workspace-setup");

/** A protection host whose computer is `computer` until the test changes it, with each command held. */
function scripted(computer: Protection, over: Partial<ProtectionView> = {}) {
  const answers: Answer[] = [];
  const state = { computer };
  const host: FakeProtectionHost = {
    view: () => view(state.computer, over),
    command: () => new Promise<void>((resolve, reject) => answers.push({ resolve, reject })),
  };
  return { host, answers, state };
}

const signInNow = async (t: FakeTransport) => {
  await play(t, signIn(CLAUDE, SIGNED_IN));
  for (let i = 0; i < 3; i += 1) await settle();
};

test.each([
  ["protected", { view: () => view(PROT.b) }],
  [
    "the query refused",
    {
      view: () => {
        throw { code: "command.invalid", message: "not admitted", retryable: false };
      },
    },
  ],
  ["no protection wire", undefined],
] as const)("%s: no step, and the first read starts after the sign-in", async (_c, protection) => {
  const t = await mount(protection as FakeProtectionHost | undefined);
  await signInNow(t);
  expect(step()).toBeNull();
  expect(reads(t)).toBe(1);
});

test("needs_set_up on Windows: the Windows screen, which is the confirm; the first read waits", async () => {
  const s = scripted(PROT.needs);
  const t = await mount(s.host);
  await signInNow(t);
  expect(step()?.dataset["state"]).toBe("ask");
  expect(step()?.textContent).toContain(
    fl("en", "Windows asks once for permission to set up the Safe workspace."),
  );
  expect(step()?.textContent).toContain("It will say the publisher is unknown");
  expect(t.protectionCommands()).toEqual([]);
  expect(reads(t)).toBe(0);
  await press(byId("protection-setup"));
  await settle();
  expect(t.protectionCommands()).toEqual(["retry_protection"]);
  expect(step()?.dataset["state"]).toBe("waiting");
  expect(byId("protection-wait")?.textContent).toBe(
    pw("en", "Choose Yes in the window Windows shows."),
  );
  expect(reads(t)).toBe(0);
  s.state.computer = PROT.b;
  await act(async () => s.answers[0]!.resolve());
  for (let i = 0; i < 3; i += 1) await settle();
  expect(step()).toBeNull();
  expect(reads(t)).toBe(1);
});

test("Not now is an outcome: the line says Not protected in Claude's words, with Try again, and the first read starts once", async () => {
  const s = scripted(PROT.needs);
  const t = await mount(s.host);
  await signInNow(t);
  await press(byId("workspace-not-now"));
  for (let i = 0; i < 3; i += 1) await settle();
  expect(t.protectionCommands()).toEqual([]);
  expect(step()?.dataset["state"]).toBe("off");
  expect(step()?.textContent).toContain(
    tbt("en", "Clauseo asks before each change in this matter."),
  );
  expect(byId("protection-failed")).toBeNull();
  expect(reads(t)).toBe(1);
  // Try again goes back to the Windows screen; the first read isn't started again
  await press(byId("workspace-retry"));
  await settle();
  expect(step()?.dataset["state"]).toBe("ask");
  expect(reads(t)).toBe(1);
});

test.each(["sandbox.setup_refused", "sandbox.setup_failed"])(
  "a %s answer is an outcome: its words on the line, and the first read starts once",
  async (code) => {
    const s = scripted(PROT.needs);
    const t = await mount(s.host);
    await signInNow(t);
    await press(byId("protection-setup"));
    s.state.computer = code === "sandbox.setup_refused" ? PROT.refused : PROT.failed;
    await act(async () => s.answers[0]!.reject({ code, message: "no", retryable: true }));
    for (let i = 0; i < 3; i += 1) await settle();
    expect(step()?.dataset["state"]).toBe("off");
    expect(byId("protection-failed")?.textContent).toBe(errorWords("en", code));
    expect(reads(t)).toBe(1);
    await signInNow(t);
    expect(reads(t)).toBe(1);
  },
);

test("where R17 says the set-up asks no administrator, it is sent once on entering, with the wait", async () => {
  const s = scripted(PROT.needs, { retry_asks_admin: false } as Partial<ProtectionView>);
  const t = await mount(s.host);
  await signInNow(t);
  expect(t.protectionCommands()).toEqual(["retry_protection"]);
  expect(step()?.dataset["state"]).toBe("wait");
  expect(byId("protection-wait")?.textContent).toBe(pw("en", "Setting up the Safe workspace."));
  s.state.computer = PROT.b;
  await act(async () => s.answers[0]!.resolve());
  for (let i = 0; i < 3; i += 1) await settle();
  expect(step()).toBeNull();
  expect(t.protectionCommands()).toEqual(["retry_protection"]);
  expect(reads(t)).toBe(1);
});

test("can't or a full pool: no step", async () => {
  const t = await mount({ view: () => view(PROT.unsupported) });
  await signInNow(t);
  expect(step()).toBeNull();
  expect(reads(t)).toBe(1);
  expect(localStorage.getItem(MARKS_KEY)).not.toBeNull();
});
