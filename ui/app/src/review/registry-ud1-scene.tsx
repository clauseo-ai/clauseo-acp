// UD1's scenes and the fake host each plays in, apart from registry-ud1.tsx to keep it under the size limit: the room
// over a FakeTransport that carries the setup wire, the first screen (no matter yet) or matter A open.
import type { SettingsView, SetupEvent } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { MATTER } from "./hosts.ts";
import { RoomDriven, type DrivenStep } from "./registry-ud1-switching.tsx";
import { RoomClicking, RoomOver } from "./states.tsx";

export type Scene = {
  /** No matter: the first screen. */
  first?: boolean;
  events: SetupEvent[];
  platform?: "mac" | "windows" | "linux";
  keys?: boolean;
  locked?: boolean;
  /** Controls to press, in order (CSS selectors). */
  clicks?: string[];
  /** The code desktop refuses each setup command with, and the steps that send one (RoomDriven). */
  refuse?: string;
  steps?: DrivenStep[];
  /** The host's folder picker and command answers, where the scene presses Open the folder. */
  open?: Pick<
    NonNullable<ConstructorParameters<typeof FakeTransport>[0]>,
    "pickFolder" | "dispatch"
  >;
};

/** The room over a fake host with the setup wire, in the scene. */
function host(s: Scene): () => FakeTransport {
  return () =>
    new FakeTransport({
      platform: s.platform ?? "mac",
      openSettings: () => {},
      ...s.open,
      query: (query) => {
        if (query === "list_matters") return { matters: s.first === true ? [] : [MATTER] };
        if (query === "get_settings")
          return {
            settings: {
              ui: {
                theme: "system",
                font_size: 16,
                language: "en",
                lite: "automatic",
                motion: "system",
              },
              privacy: { counts: true, crash_reports: false, counts_started: false },
              locked: s.locked === true ? ["privacy.counts"] : [],
            } as unknown as SettingsView,
          };
        if (typeof query === "object" && "list_threads" in query) return { threads: [] };
        throw new Error(`not scripted: ${JSON.stringify(query)}`);
      },
      setup: {
        first: () => [{ setup_info: { keys_available: s.keys ?? true } }, ...s.events],
        command: () => {
          if (s.refuse !== undefined)
            throw { code: s.refuse, message: "refused", retryable: true } as const;
        },
      },
    });
}

/** The entry's render: the room over the scene's host, pressed or driven as the scene says. */
export const render = (s: Scene) =>
  s.steps !== undefined
    ? () => <RoomDriven host={host(s)} steps={s.steps ?? []} />
    : s.clicks === undefined
      ? () => <RoomOver host={host(s)} />
      : () => <RoomClicking host={host(s)} clicks={s.clicks ?? []} />;
