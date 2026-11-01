// The page's half of the compile-time switch for the window's own frame: the room build (VITE_CLAUSEO_WINDOW=room)
// draws the title bar itself. The desktop crate reads the same variable at compile time for the host's half
// (decorations off on Windows, the Mac's overlay title bar).

/** Whether this build draws the window's frame: the toolbar is the title bar. */
export const OWN_FRAME: boolean = import.meta.env.VITE_CLAUSEO_WINDOW === "room";
