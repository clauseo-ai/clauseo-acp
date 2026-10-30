import { MatterContext } from "@clauseo/components/clauseo/matter-context.ts";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { LayerProvider } from "@clauseo/components/ui/layer.tsx";
import { ToastProvider } from "@clauseo/components/ui/toast.tsx";
import { EdgeParts } from "./edge.tsx";
import { MATTER } from "./matter.ts";
import { MattersParts } from "./matters.tsx";
import { ReaderParts } from "./reader.tsx";
import { RoomParts } from "./room.tsx";
import { VersionsParts } from "./versions.tsx";

/** Every one of Clauseo's parts in its main states, grouped as the room arranges them, over an invented matter. */
export function PartsPage() {
  return (
    <ToastProvider>
      <LayerProvider>
        <MatterContext value={MATTER}>
          <RoomScopeProvider>
            <div className="grid content-start gap-s8 min-h-screen bg-ground text-primary px-s6 pt-s6 pb-s10">
              <h1 lang="en" className="font-en text-ui text-primary">
                Clauseo parts
              </h1>
              <RoomParts />
              <MattersParts />
              <VersionsParts />
              <EdgeParts />
              <ReaderParts />
            </div>
          </RoomScopeProvider>
        </MatterContext>
      </LayerProvider>
    </ToastProvider>
  );
}
