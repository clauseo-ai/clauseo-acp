// The desktop's word on whether the main window is in the OS's own full screen (transport/full-screen-feed.ts), for
// the toolbar's `fullScreen`. Undefined until the desktop has said, and with a transport that carries no such word (the
// probe without one, the copy review): the toolbar then reads the window's size against the screen's, as before.
import { useEffect, useState } from "react";
import { isFullScreenSource } from "../transport/full-screen-feed.ts";

/** The desktop's last full-screen value for the main window, or undefined before it has sent one. */
export function useHostFullScreen(transport: object): boolean | undefined {
  const [fullScreen, setFullScreen] = useState<boolean | undefined>(undefined);
  useEffect(() => {
    if (!isFullScreenSource(transport)) return undefined;
    const stop = transport.onFullScreen(setFullScreen);
    return () => {
      stop();
      setFullScreen(undefined);
    };
  }, [transport]);
  return fullScreen;
}
