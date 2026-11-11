// @vitest-environment happy-dom
// UC's Safe workspace step in the first run, over a FakeTransport with the setup and protection
// wires. Design B: the installer sets the Safe workspace up, so a computer protected when the app
// first opens, or a refused protection query, has no step, and the first read is offered at once (D-015: offered,
// never sent by itself). Where
// the installer's admin copy is missing (admin_copy_missing), the step is the repair, with "Get the
// installer" on a per-user install. needs_set_up (the copy there, after an in-app Remove) keeps the
// app's set-up: the Windows screen, which is itself the confirm, where it asks an administrator;
// sent once on entering, with the wait, where it doesn't. "Not now", a refusal and a failure each
// count as the step's outcome, the room's line then says Not protected, and UD2's first read starts
// after it, once.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { INSTALLER_CLOSE, pw } from "@clauseo/components/lib/words-protection.ts";
import { APP_VERSION } from "../../settings/version.ts";
import { installerPage } from "../protection/installer.ts";
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
import { NO_PROMPT, PROT, view } from "../protection/fixtures.ts";
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
    openSettings: () => {},
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
/** The first read offered (D-015: never sent by itself): 1 while Read the papers stands, unpressed, and nothing sent. */
const offered = (t: FakeTransport) => (byId("first-read-offer") !== null && reads(t) === 0 ? 1 : 0);
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
  expect(offered(t)).toBe(1);
  // pressed, it starts once
  await press(byId("first-read-offer"));
  for (let i = 0; i < 3; i += 1) await settle();
  expect(reads(t)).toBe(1);
});

test("set up at install (design B): protected when the app first opens, so no step, no command and no prompt", async () => {
  const s = scripted(PROT.b, { set_up_at: 1_762_400_000_000 } as Partial<ProtectionView>);
  const t = await mount(s.host);
  await signInNow(t);
  expect(step()).toBeNull();
  expect(byId("protection-setup")).toBeNull();
  expect(t.protectionCommands()).toEqual([]);
  expect(offered(t)).toBe(1);
});

test("admin_copy_missing: the repair's words and Get the installer, no command; the first read waits for Not now", async () => {
  const s = scripted(PROT.missing);
  const t = await mount(s.host);
  await signInNow(t);
  expect(step()?.dataset["state"]).toBe("repair");
  expect(byId("protection-repair")?.textContent).toBe(
    `${pw("en", "The Safe workspace wasn't set up when Clauseo was installed.")} ${pw(
      "en",
      "Run the Clauseo installer again to set up the Safe workspace.",
    )}`,
  );
  expect(byId("protection-setup")).toBeNull();
  expect(byId("protection-close-line")?.textContent).toBe(pw("en", INSTALLER_CLOSE));
  expect(reads(t)).toBe(0);
  await press(byId("protection-installer"));
  await settle();
  expect(t.only("open_external")).toEqual([
    { call: "open_external", url: installerPage(APP_VERSION) },
  ]);
  expect(t.only("quit_app")).toEqual([{ call: "quit_app" }]);
  expect(t.protectionCommands()).toEqual([]);
  await press(byId("workspace-not-now"));
  for (let i = 0; i < 3; i += 1) await settle();
  expect(step()?.dataset["state"]).toBe("off");
  expect(byId("workspace-retry")).toBeNull();
  expect(t.protectionCommands()).toEqual([]);
  expect(offered(t)).toBe(1);
});

test("admin_copy_missing on a firm install: the firm manages the install, with no button", async () => {
  const s = scripted(PROT.missing, { unprotected_auto_allowed: false });
  const t = await mount(s.host);
  await signInNow(t);
  expect(step()?.dataset["state"]).toBe("repair");
  expect(byId("protection-repair")?.textContent).toContain(
    pw("en", "Your firm manages how Clauseo is installed on this computer."),
  );
  expect(byId("protection-installer")).toBeNull();
  expect(t.protectionCommands()).toEqual([]);
});

test("needs_set_up with retry_asks_admin true (the admin copy there, after an in-app Remove): the Windows screen, which is the confirm", async () => {
  const s = scripted(PROT.needs, { retry_asks_admin: true } as Partial<ProtectionView>);
  const t = await mount(s.host);
  await signInNow(t);
  expect(step()?.dataset["state"]).toBe("ask");
  expect(step()?.textContent).toContain(
    fl("en", "Windows asks once for permission to set up the Safe workspace."),
  );
  expect(step()?.textContent).toContain(
    pw(
      "en",
      "Windows will ask to let Clauseo set up the Safe workspace. It will say the publisher is unknown. That's expected for Clauseo. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.",
    ),
  );
  expect(byId("protection-repair")).toBeNull();
  expect(t.protectionCommands()).toEqual([]);
  expect(reads(t)).toBe(0);
  await press(byId("protection-setup"));
  await settle();
  expect(t.protectionCommands()).toEqual(["retry_protection"]);
  expect(step()?.dataset["state"]).toBe("waiting");
  expect(byId("protection-wait")?.textContent).toBe(
    pw("en", "Choose Yes in the window Windows shows."),
  );
  s.state.computer = PROT.b;
  await act(async () => s.answers[0]!.resolve());
  for (let i = 0; i < 3; i += 1) await settle();
  expect(step()).toBeNull();
  expect(offered(t)).toBe(1);
});

test("any other block: no step; the room's line and note say it", async () => {
  const t = await mount({ view: () => view(PROT.policy) });
  await signInNow(t);
  expect(step()).toBeNull();
  expect(offered(t)).toBe(1);
});

test("Settings on the Not protected line opens the window at #protection", async () => {
  const s = scripted(PROT.needs);
  const t = await mount(s.host);
  await signInNow(t);
  await press(byId("workspace-not-now"));
  for (let i = 0; i < 3; i += 1) await settle();
  await press(byId("workspace-settings"));
  expect(t.only("open_settings")).toEqual([{ call: "open_settings", section: "protection" }]);
});

test("Not now is an outcome: the line says Not protected in Claude's words, and the first read starts once", async () => {
  const s = scripted(PROT.needs);
  const t = await mount(s.host);
  await signInNow(t);
  await press(byId("workspace-not-now"));
  for (let i = 0; i < 3; i += 1) await settle();
  expect(t.protectionCommands()).toEqual([]);
  expect(step()?.dataset["state"]).toBe("off");
  expect(step()?.textContent).toContain(
    tbt(
      "en",
      "Clauseo changes files in this matter without asking, and asks before changing files outside it.",
    ),
  );
  expect(byId("protection-failed")).toBeNull();
  expect(offered(t)).toBe(1);
  // Try again goes back to the Windows screen; the first read isn't started again
  await press(byId("workspace-retry"));
  await settle();
  expect(step()?.dataset["state"]).toBe("ask");
  expect(reads(t)).toBe(0);
});

test.each(["sandbox.setup_refused", "sandbox.setup_failed"])(
  "a %s answer is an outcome: its words on the line, and the first read starts once",
  async (code) => {
    const s = scripted(PROT.needs, NO_PROMPT);
    const t = await mount(s.host);
    await signInNow(t);
    expect(t.protectionCommands()).toEqual(["retry_protection"]);
    s.state.computer = code === "sandbox.setup_refused" ? PROT.refused : PROT.failed;
    await act(async () => s.answers[0]!.reject({ code, message: "no", retryable: true }));
    for (let i = 0; i < 3; i += 1) await settle();
    expect(step()?.dataset["state"]).toBe("off");
    expect(byId("protection-failed")?.textContent).toBe(errorWords("en", code));
    expect(offered(t)).toBe(1);
    await signInNow(t);
    expect(offered(t)).toBe(1);
    // Try again goes back to the set-up's screen, sending nothing until it is pressed
    await press(byId("workspace-retry"));
    await settle();
    expect(step()?.dataset["state"]).toBe("ask");
    expect(t.protectionCommands()).toEqual(["retry_protection"]);
    expect(reads(t)).toBe(0);
  },
);

test("where R17 says the set-up asks no administrator, it is sent once on entering, with the wait", async () => {
  const s = scripted(PROT.needs, NO_PROMPT);
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
  expect(offered(t)).toBe(1);
});

test("can't or a full pool: no step", async () => {
  const t = await mount({ view: () => view(PROT.unsupported) });
  await signInNow(t);
  expect(step()).toBeNull();
  expect(offered(t)).toBe(1);
  expect(localStorage.getItem(MARKS_KEY)).not.toBeNull();
});
