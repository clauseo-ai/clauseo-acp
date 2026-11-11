// R23's updater, scripted for FakeTransport's `update` (decision record 0037): the `get_update` query and the four
// commands. A command the host answers with a view is followed by `update_changed` carrying it, to every app listener,
// as the host sends it (which needs `host.next`'s app part); `FakeTransport.updateChanged` plays one at any time.
// `fakeUpdate` is a stateful host that moves as decision record 0037's table does, and refuses as scripted.
import type { UpdateCommand } from "../store/update.ts";
import type { ErrorCode, HostCommand, UpdateView } from "./protocol.ts";

export type FakeUpdateHost = {
  /** Answers each `get_update`; throwing refuses it. */
  view: () => UpdateView | Promise<UpdateView>;
  /** Answers each update command; throwing refuses it. A view it gives is played as `update_changed`. */
  command?: (command: UpdateCommand) => UpdateView | void | Promise<UpdateView | void>;
};

/** Whether a command is one of the updater's. */
export const isUpdateCommand = (command: HostCommand): command is UpdateCommand =>
  command.check_for_update !== undefined ||
  command.install_update !== undefined ||
  command.cancel_scheduled_update !== undefined ||
  command.set_update_mode !== undefined;

/** The refusals a fake can script, by the command's wire name. */
export type FakeUpdateRefusals = Partial<
  Record<
    "check_for_update" | "install_update" | "cancel_scheduled_update" | "set_update_mode",
    ErrorCode
  >
>;

const refusal = (code: ErrorCode) => ({
  code,
  message: "the fake's scripted refusal",
  retryable: false,
});

/** A stateful updater from `view`: a check ends idle (checked now), `install_update` installs now or is scheduled,
 * Cancel takes it back to ready, and the mode is set. A command its state doesn't take is `command.invalid`, as the
 * host's is; `refuse` names a command's scripted refusal ahead of that. */
export function fakeUpdate({
  view,
  refuse = {},
  now = () => Date.now(),
}: {
  view: UpdateView;
  refuse?: FakeUpdateRefusals;
  now?: () => number;
}): FakeUpdateHost & { current: () => UpdateView } {
  let at = view;
  const versionOf = (): string | null => {
    const s = at.state;
    if (typeof s !== "object") return null;
    return s.ready?.version ?? s.scheduled?.version ?? null;
  };
  return {
    current: () => at,
    view: () => at,
    command: (command) => {
      const name = Object.keys(command)[0] as keyof FakeUpdateRefusals;
      const code = refuse[name];
      if (code !== undefined) throw refusal(code);
      const s = at.state;
      const version = versionOf();
      if (command.check_for_update !== undefined) {
        if (s !== "idle" && !(typeof s === "object" && s.failed !== undefined))
          throw refusal("command.invalid");
        at = { ...at, state: "idle", checked_at: now() };
      } else if (command.install_update !== undefined) {
        if (typeof s !== "object" || s.ready === undefined || version === null)
          throw refusal("command.invalid");
        at = {
          ...at,
          state:
            command.install_update.when === "now"
              ? { installing: { version } }
              : { scheduled: { version } },
        };
      } else if (command.cancel_scheduled_update !== undefined) {
        if (typeof s !== "object" || s.scheduled === undefined || version === null)
          throw refusal("command.invalid");
        at = { ...at, state: { ready: { version } } };
      } else if (command.set_update_mode !== undefined) {
        if (command.set_update_mode.mode === "none") throw refusal("command.invalid");
        at = { ...at, mode: command.set_update_mode.mode };
      }
      return at;
    },
  };
}
