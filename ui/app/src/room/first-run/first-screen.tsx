// The first screen (UD1), apart from first-run.tsx to keep it under the size limit: the question, and both plans side
// by side at its foot, Claude's working line then ChatGPT's, each with its own "Sign in with {name}". A press makes that
// plan the chosen one and starts its install and sign-in at once, beside the other plan's download; only its sign-in
// waits, for the other plan's sign-in under way (the setup store's queue: one sign-in at a time, and nothing dropped).
// Open the folder asks first about a folder whose path has a backslash, on macOS and Linux (FR30, folder-backslash.ts),
// as New matter and Locate do: Choose another folder, or Open anyway. And about a folder New matter would warn about, on
// a network drive or a drive without file permissions (first-folder-check.ts, first-folder-step.tsx): the same two
// buttons. A folder a matter on the list already uses opens that matter.
import { lazy, Suspense, useEffect, useLayoutEffect, useRef, useState } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { Welcome, type FootPlan } from "@clauseo/components/clauseo/first-launch/welcome.tsx";
import { ToolsLine } from "@clauseo/components/clauseo/first-launch/tools.tsx";
import { useDisplay, type Platform } from "@clauseo/components/lib/display.ts";
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import { toHostError, type HostTransport } from "../../transport/host-transport.ts";
import type { FolderRefId } from "../../transport/protocol.ts";
import { useOnline } from "../../transport/online.ts";
import {
  PLANS,
  isReady,
  isSignedIn,
  planInPlay,
  type AgentSetup,
  type SetupState,
  type SetupStore,
} from "../../store/setup.ts";
import type { Matters } from "../matters.ts";
import { hasBackslash } from "../folder-backslash.ts";
import { usedBy, warningOf, type HeldFolder } from "../first-folder-check.ts";
import type { HeldPick } from "../locate-held.ts";
import type { Refused } from "../new-matter.tsx";
import { usePickWait } from "../picker-wait.ts";
import { markButton, markWelcome } from "../../start-marks.ts";
import { IDS, NAME, planOf } from "./acts.ts";
import { readyPlan, toolsView } from "./views.ts";
import { browserNote, footActs, welcomeFoot } from "./welcome-foot.tsx";

/** U2's new matter form in place, and a refused folder's words: New matter's chunk (new-matter.tsx), loaded as the
 * first screen needs them. */
const loadNewMatter = () => late(import("../new-matter.tsx"));
const FirstMatter = lazy(() => loadNewMatter().then((m) => ({ default: m.FirstMatter })));
const RefusedLine = lazy(() => loadNewMatter().then((m) => ({ default: m.RefusedLine })));
const RefusedOpen = lazy(() => loadNewMatter().then((m) => ({ default: m.RefusedOpen })));
/** The step for a folder Claude can't start in (FR30): Locate's (locate-warning.tsx), loaded when a pick needs it. */
const BackslashStep = lazy(() =>
  late(import("../locate-warning.tsx")).then((m) => ({ default: m.LocateWarning })),
);
/** The step for a folder New matter would warn about (first-folder-step.tsx), loaded when a pick needs it. */
const FolderStep = lazy(() =>
  late(import("../first-folder-step.tsx")).then((m) => ({ default: m.FirstFolderStep })),
);

/** The folder's own name, the last part of how it reads: on macOS and Linux a backslash is part of a name, never a
 * separator. */
const folderName = (display: string, platform: Platform) =>
  display
    .split(platform === "windows" ? /[\\/]/ : "/")
    .filter((p) => p !== "")
    .pop() ?? display;

/** Whether the plan's sign-in state is a saved key the keychain didn't give. */
const keyUnreadable = (setup: AgentSetup | undefined): boolean => {
  const signIn = setup?.signIn;
  return typeof signIn === "object" && signIn.failed?.code === "agent.key_unreadable";
};

/** A row without its note: the first screen says the note under the row instead. */
function unnoted(row: WorkingState | null): WorkingState | null {
  if (row?.kind !== "wait" || row.note === undefined) return row;
  const { note: _note, ...rest } = row;
  return rest;
}

/** The first screen: the question, both plans at its foot, the counts switch. Its acts make the first matter: a
 * folder chosen opens it at once (the room opens the moment it is made); a new matter takes U2's form, in place. */
export function FirstScreen({
  store,
  matters,
  transport,
  state,
}: {
  store: SetupStore;
  matters: Matters;
  transport: HostTransport;
  state: SetupState;
}) {
  const { lang, platform } = useDisplay();
  const online = useOnline();
  const plans: FootPlan[] = PLANS.map((agent) => {
    const name = NAME[planOf(agent)];
    const setup = state.agents[agent];
    // a row's note (an unreadable key's) goes under the row, in Welcome's
    // own slot, not in it
    const row = welcomeFoot(
      lang,
      agent,
      name,
      setup,
      state,
      online,
      footActs(store, agent, state),
      IDS.install,
    );
    const install = unnoted(row);
    const own = row?.kind === "wait" ? (row.note ?? null) : null;
    return {
      key: agent,
      name,
      install,
      note: browserNote(lang, agent, install, setup) ?? own,
      // an unreadable key's row offers Try again, which reads the key, never a sign-in
      signs: !keyUnreadable(setup),
      // ready here, or installed and signed in wherever that happened (D-012: Settings' sign-in left this window
      // with no row for the plan, since its install was never seen turning ready here)
      ready:
        setup?.readyHere === true ||
        ((isReady(setup?.install) || setup?.install === "managed") && isSignedIn(setup?.signIn)),
    };
  });
  // the papers line names the one plan in play, else both
  const inPlay = planInPlay(state);
  // the document tools, under the two rows: their size before, their download after a plan is ready
  const tools = toolsView(state, online);
  const retryThrough = readyPlan(state);
  const [form, setForm] = useState(false);
  const [refused, setRefused] = useState<Refused | null>(null);
  // the folder picked is being made a matter: "Opening the folder…", and Open the folder waits (a second press does
  // nothing) until the host answers
  const [making, setMaking] = useState(false);
  // a folder picked whose path has a backslash, held for its step (FR30): Choose another folder, or Open anyway
  const [held, setHeld] = useState<HeldPick | null>(null);
  // a folder picked on a network drive or a drive without file permissions, held for its step
  const [warned, setWarned] = useState<HeldFolder | null>(null);
  const makingNow = useRef(false);
  const wait = usePickWait();
  useEffect(() => store.count({ onboarding_step: { step: "welcome_shown" } }), [store]);
  // the start-up's marks (start-marks.ts): the welcome's first commit, then its button, which sends them all once
  useLayoutEffect(markWelcome, []);
  const openFolder = async () => {
    if (makingNow.current) return;
    setRefused(null);
    const picked = await wait.track(() => matters.pickFolder());
    if (picked === null || makingNow.current) return;
    if (hasBackslash(picked.display, platform)) {
      // Locate's step first; its Open anyway asks the drive check too, a drive's step then, else the folder is made
      setHeld({ display: picked.display, again: openFolder, anyway: () => checkThenMake(picked) });
      return;
    }
    await checkThenMake(picked);
  };
  /** The drive check, then the folder: a network drive or a drive without permissions holds the pick for its step,
   * else the folder made. Open the folder waits meanwhile, "Opening the folder…" under it, and a second press does
   * nothing. */
  const checkThenMake = async (picked: { folder: FolderRefId; display: string }) => {
    if (makingNow.current) return;
    makingNow.current = true;
    setMaking(true);
    const kind = await warningOf(matters, picked.folder);
    makingNow.current = false;
    setMaking(false);
    if (kind !== null) {
      setWarned({ display: picked.display, kind, again: openFolder, anyway: () => make(picked) });
      return;
    }
    await make(picked);
  };
  /** The folder picked made a matter. */
  const make = async (picked: { folder: FolderRefId; display: string }) => {
    if (makingNow.current) return;
    store.firstAct("folder_chosen");
    makingNow.current = true;
    setMaking(true);
    try {
      // made, or a matter on the list that already uses the folder (MatterCreated naming it): the room opens it, its
      // name as it was
      await matters.create(folderName(picked.display, platform), {
        existing_folder: { folder: picked.folder },
      });
    } catch (thrown) {
      const { code, matter } = toHostError(thrown);
      // a matter on the list already uses the folder (matter.folder_is_matter, naming it): the lawyer asked to open
      // that folder's matter, so it opens, nothing made or renamed
      const used = await usedBy(matters, { code, matter });
      if (used !== null) matters.open(used);
      else setRefused({ code, path: picked.display, ...(matter === undefined ? {} : { matter }) });
    } finally {
      makingNow.current = false;
      setMaking(false);
    }
  };
  const openSettings = transport.openSettings;
  return (
    <>
      {held !== null && (
        <Suspense fallback={null}>
          <BackslashStep
            held={held}
            onClose={() => setHeld(null)}
            anyway="Open anyway"
            testId="first-backslash"
          />
        </Suspense>
      )}
      {warned !== null && (
        <Suspense fallback={null}>
          <FolderStep held={warned} onClose={() => setWarned(null)} />
        </Suspense>
      )}
      <Welcome
        plans={plans}
        papersPlan={inPlay === null ? null : NAME[planOf(inPlay)]}
        tools={
          tools === null ? null : (
            <ToolsLine
              view={tools}
              onRetry={
                retryThrough === null
                  ? undefined
                  : () => store.command({ retry_agent_install: { agent: retryThrough } })
              }
            />
          )
        }
        counts={{ ...state.counts, onChange: store.setCounts }}
        onOpenFolder={() => void openFolder()}
        opening={wait.opening}
        making={making}
        onNewMatter={() => {
          setRefused(null);
          setForm(true);
        }}
        {...(form
          ? {
              form: (
                <Suspense fallback={null}>
                  <FirstMatter
                    matters={matters}
                    onAct={() => store.firstAct("new_matter")}
                    onBack={() => setForm(false)}
                  />
                </Suspense>
              ),
            }
          : {})}
        error={
          refused === null ? null : (
            <Suspense fallback={null}>
              <RefusedLine matters={matters} refused={refused} />
            </Suspense>
          )
        }
        errorAction={
          refused === null ? null : (
            <Suspense fallback={null}>
              <RefusedOpen matters={matters} refused={refused} />
            </Suspense>
          )
        }
        onWhatsSent={
          openSettings === undefined
            ? undefined
            : // what the anonymous counts include: Settings opens at Privacy (D-003)
              () => void openSettings.call(transport, "privacy").catch(() => {})
        }
        testIds={{ counts: IDS.counts, open: "open-folder", newMatter: "start-new-matter" }}
        openRef={(button) => markButton(button, transport.pageMarks?.bind(transport))}
      />
    </>
  );
}
