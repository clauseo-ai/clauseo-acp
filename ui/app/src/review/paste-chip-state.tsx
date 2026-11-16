// The composer with a long paste's chip, for the paste chip's copy review entries (registry-uc-paste-chip.tsx).
import { Composer } from "@clauseo/components/clauseo/composer.tsx";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { useLayoutEffect } from "react";

/** The composer at the window's foot, its draft a chip of `text`; with `focus`, the chip's remove button focused. */
export function Pasted({ text, focus = false }: { text: string; focus?: boolean }) {
  useLayoutEffect(() => {
    if (focus) document.querySelector<HTMLButtonElement>(".q-paste .q-paper-remove")?.focus();
  }, [focus]);
  return (
    <RoomScopeProvider>
      <div className="flex min-h-dvh flex-col justify-end">
        <Composer took={null} plan="" hint={null} initialText={{ typed: "", pastes: [text] }} />
      </div>
    </RoomScopeProvider>
  );
}
