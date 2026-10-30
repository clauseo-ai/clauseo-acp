// Each sample is its own room: its nodes, its hand and its ids, so two samples of one part never share an element or
// an id on the page. The headings inside it sit one level under the sample's name.
import type { ReactNode } from "react";
import { HeadingLevel } from "@clauseo/components/clauseo/heading.tsx";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";

export function Own({ children }: { children: ReactNode }) {
  return (
    <RoomScopeProvider>
      <HeadingLevel level={4}>{children}</HeadingLevel>
    </RoomScopeProvider>
  );
}
