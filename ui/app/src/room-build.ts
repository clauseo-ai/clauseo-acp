// The one spelling of the build switch. VITE_CLAUSEO_WINDOW=room makes the room build: the page mounts the room
// (src/room/) instead of R12's internal window, and the room's window draws its own frame, the toolbar as its title bar.
// The desktop crate reads the same variable at compile time for the host's half of that frame (decorations off on
// Windows, the Mac's overlay title bar). A compile-time constant, so the default build drops the room and all it
// imports. App.tsx spells the same test inline, so Rolldown can drop the room's import (room-build.test.ts).

/** Whether this is the room build. */
export const ROOM_BUILD: boolean = import.meta.env.VITE_CLAUSEO_WINDOW === "room";
