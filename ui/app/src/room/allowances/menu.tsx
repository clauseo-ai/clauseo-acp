// R18, the open matter's menu on the rail (main window only: Settings has no rail): "Switch off the
// Safe workspace…", which asks the host for its native confirm, or, once it is off, "Switch the Safe
// workspace back on", which needs none; and "Use files from another matter…", which opens the list
// of the other matters (other-matter.tsx, loaded on the press), whose pick asks the host for its
// picker and dialog. Switch-off shows only on a protected matter, and never while the firm requires
// the Safe workspace or forbids working without it (the protection view's `required` and
// `unprotected_auto_allowed`); the host refuses it then anyway, and a refusal's words stand on the
// row's note line until the next press (a switch-off refused with `sandbox.required` says its own
// line, not the start-up words). "Remembered answers…", while the matter has any (its header on
// the matter's stream, held only while the menu is open), opens Settings at Remembered answers.
import { aw } from "@clauseo/components/lib/words-allowances.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { MenuItem } from "@clauseo/components/ui/menu.tsx";
import { useState, useSyncExternalStore, type ReactNode } from "react";
import { protectionStoreOf } from "../../store/protection.ts";
import type { ViewStore } from "../../store/store.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterId } from "../../transport/protocol.ts";
import type { MatterInfo } from "../host-next.ts";
import { notProtectedCause } from "../protection/causes.ts";
import { filesFrom, switchOff, switchOn } from "./commands.ts";
import { LazyOtherMatter } from "./other-matter-load.ts";
import { RememberedItem } from "./remembered-item.tsx";

export type MatterSafety = {
  /** The open matter's items, for its menu; undefined when it has none. */
  items: ReactNode | undefined;
  /** The last refusal's words for `matter`, for the row's note line; null when none stands. */
  failedFor: (matter: MatterId) => string | null;
  /** The list of the other matters, while it is open. */
  dialog: ReactNode;
};

/** Whether the firm lets a matter switch the Safe workspace off, by the protection view; true while it isn't read. */
const offAllowed = (
  view: { required: boolean; unprotected_auto_allowed: boolean } | null | undefined,
) => view?.required !== true && view?.unprotected_auto_allowed !== false;

export function useMatterSafety(
  transport: HostTransport,
  open: MatterInfo | null,
  matters: readonly MatterInfo[],
  lang: Lang,
  views: ViewStore,
): MatterSafety {
  const [store] = useState(() => protectionStoreOf(transport));
  const readAllowed = () => offAllowed(store.state().view);
  const allowed = useSyncExternalStore(store.subscribe, readAllowed, readAllowed);
  const [failed, setFailed] = useState<{ matter: MatterId; code: string; off: boolean } | null>(
    null,
  );
  const openAt = transport.openSettings;
  const [picking, setPicking] = useState<MatterId | null>(null);
  const run = (matter: MatterId, act: () => Promise<string | null>, off = false) => {
    setFailed(null);
    void act().then((code) => setFailed(code === null ? null : { matter, code, off }));
  };
  const others =
    open === null ? [] : matters.filter((m) => m.matter !== open.matter && m.kind !== "scratch");
  let items: ReactNode | undefined;
  if (open !== null && open.kind !== "scratch") {
    const matter = open.matter;
    const cause = notProtectedCause(open.protection);
    const off = cause?.group === "off";
    const on = open.protection !== undefined && "protected" in open.protection;
    items = (
      <>
        {off ? (
          <MenuItem
            label={aw(lang, "Switch the Safe workspace back on")}
            onClick={() => run(matter, () => switchOn(transport, matter))}
          />
        ) : on && allowed ? (
          <MenuItem
            label={aw(lang, "Switch off the Safe workspace…")}
            onClick={() => run(matter, () => switchOff(transport, matter), true)}
          />
        ) : null}
        {openAt !== undefined && (
          <RememberedItem
            views={views}
            matter={matter}
            lang={lang}
            onOpen={() => void openAt("remembered_answers").catch(() => {})}
          />
        )}
        {others.length > 0 && (
          <MenuItem
            label={aw(lang, "Use files from another matter…")}
            onClick={() => {
              setFailed(null);
              setPicking(matter);
            }}
          />
        )}
      </>
    );
    if (!off && !(on && allowed) && others.length === 0 && openAt === undefined) items = undefined;
  }
  const dialog =
    picking === null ? null : (
      <LazyOtherMatter
        matters={matters.filter((m) => m.matter !== picking && m.kind !== "scratch")}
        onPick={(from) => {
          const matter = picking;
          setPicking(null);
          run(matter, () => filesFrom(transport, matter, from));
        }}
        onClose={() => setPicking(null)}
      />
    );
  return {
    items,
    failedFor: (matter) =>
      failed === null || failed.matter !== matter
        ? null
        : failed.off && failed.code === "sandbox.required"
          ? aw(
              lang,
              "Your firm requires the Safe workspace in every matter, so it can't be switched off.",
            )
          : errorWords(lang, failed.code),
    dialog,
  };
}
