// The rows of host.next the fake answers (room/host-next.ts's NextHost parts), wired to FakeTransport's own calls.
import type {
  AppEvent,
  NextCommand,
  NextOutcome,
  NextPart,
  NextHost,
  NextQuery,
  NextQueryResult,
  Papers,
  Settings,
} from "../room/host-next.ts";
import { WIRE } from "../room/host-next.ts";
import { internalError } from "./host-transport.ts";
import type { CommandEnvelope, CommandOutcome } from "./protocol.ts";

/** What the parts reach back into FakeTransport for: its recorded `next_query` and `next_command` calls, its
 * dispatch, the listeners it keeps, and the app events `host.next.first` scripts. */
export type NextPartsSource = {
  firstApp: () => readonly AppEvent[];
  query: (query: NextQuery) => Promise<NextQueryResult>;
  command: (command: NextCommand) => Promise<NextOutcome>;
  dispatch: (command: CommandEnvelope) => Promise<CommandOutcome>;
  settingsHeard: Set<(settings: Settings) => void>;
  appHeard: Set<(event: AppEvent) => void>;
};

/** Each NextHost part the fake scripts, answered through `fake`. */
export function nextPartsOf(fake: NextPartsSource): {
  [P in NextPart]-?: NonNullable<NextHost[P]>;
} {
  return {
    papers: {
      listPapers: async (matter) => {
        const result = await fake.query({ [WIRE.listPapers]: { matter } });
        if (!("papers" in result)) throw internalError("list_papers answered without papers");
        return result.papers satisfies Papers;
      },
    },
    settings: {
      getSettings: async () => {
        const result = await fake.query(WIRE.getSettings);
        if (!("settings" in result)) throw internalError("get_settings answered without settings");
        return result.settings;
      },
      setUiSettings: async (patch) => {
        await fake.command({ [WIRE.setUiSettings]: { patch } });
      },
      setToolSettings: async (patch) => {
        await fake.command({ [WIRE.setToolSettings]: { patch } });
      },
      setPrivacySettings: async (patch) => {
        await fake.command({ [WIRE.setPrivacySettings]: { patch } });
      },
      onSettingsChanged: (listener) => {
        fake.settingsHeard.add(listener);
        return () => fake.settingsHeard.delete(listener);
      },
    },
    matters: {
      listMatters: async () => {
        const result = await fake.query(WIRE.listMatters);
        if (!("matters" in result)) throw internalError("list_matters answered without matters");
        return result.matters;
      },
      listThreads: async (matter) => {
        const result = await fake.query({ [WIRE.listThreads]: { matter } });
        if (!("threads" in result)) throw internalError("list_threads answered without threads");
        return result.threads;
      },
      command: (command) => fake.command(command),
    },
    location: {
      getDefaultLocation: async () => {
        const result = await fake.query(WIRE.getDefaultLocation);
        if (!("default_location" in result))
          throw internalError("get_default_location answered without default_location");
        return result.default_location.display;
      },
    },
    app: {
      subscribeApp: (listener) => {
        fake.appHeard.add(listener);
        const first = fake.firstApp();
        if (first.length > 0)
          queueMicrotask(() => {
            for (const event of first) if (fake.appHeard.has(listener)) listener(event);
          });
        return () => fake.appHeard.delete(listener);
      },
    },
    restart: {
      restartAgent: async (thread) => {
        await fake.dispatch({
          command_id: crypto.randomUUID(),
          command: { restart_agent: { thread } },
        });
      },
    },
    agents: {
      listAgents: async () => {
        const result = await fake.query(WIRE.listAgents);
        if (!("agents" in result)) throw internalError("list_agents answered without agents");
        return result.agents;
      },
    },
    retry: {
      retryFrom: async (thread, message) => {
        const outcome = await fake.command({ [WIRE.retryFrom]: { thread, message } });
        if (typeof outcome === "string" || !("thread_created" in outcome))
          throw internalError("retry_from answered without a thread");
        return outcome.thread_created.thread;
      },
    },
  };
}
