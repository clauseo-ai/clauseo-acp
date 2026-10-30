import { Toast } from "@base-ui/react/toast";
import type { ReactNode } from "react";
import { useUiWords } from "../lib/words-ui.ts";
import { IconTick } from "../icons/icons.tsx";
import { Button } from "./button.tsx";
import { toast, toastViewport } from "./recipes-overlays.ts";

/** Toasts, rarely: a confirmation of something done out of sight, such as "List of citations copied.", and never
 * an error in a thread, which stays inline on the working line where it happened. Queue one with useToasts().show(title)
 * (toast-queue.ts) inside this provider. A toast rises 12 px on the spring, stays 5 s while
 * the window is in view and the pointer or focus isn't on it, and leaves in 160 ms. F6 reaches it from the keyboard,
 * and Escape or Close dismisses it. */
export function ToastProvider({ children }: { children: ReactNode }) {
  return (
    <Toast.Provider timeout={5000} limit={2}>
      {children}
      <Toast.Portal>
        <Viewport />
      </Toast.Portal>
    </Toast.Provider>
  );
}

/** Where toasts stand. F6 brings focus here; Escape, from here or from a toast, dismisses the newest one, and focus
 * goes back to where it was before F6. */
function Viewport() {
  const w = useUiWords();
  const { toasts, close } = Toast.useToastManager();
  return (
    <Toast.Viewport
      aria-label={w.ui("Notifications")}
      className={toastViewport}
      onKeyDown={(e) => {
        const newest = toasts[0];
        if (e.key === "Escape" && e.target === e.currentTarget && newest !== undefined)
          close(newest.id);
      }}
    >
      <Toasts />
    </Toast.Viewport>
  );
}

/** Each toast: the check the accept mark carries, the words, and Close. */
function Toasts() {
  const { toasts } = Toast.useToastManager();
  const w = useUiWords();
  return toasts.map((t) => (
    <Toast.Root key={t.id} toast={t} className={toast}>
      <Toast.Content className="contents">
        <IconTick size={14} className="text-primary" />
        <Toast.Title className="flex-1 min-w-0 deva-nudge">
          {typeof t.title === "string" ? w.say(t.title) : t.title}
        </Toast.Title>
        <Toast.Close render={<Button kind="quiet" size="small" />}>{w.ui("Close")}</Toast.Close>
      </Toast.Content>
    </Toast.Root>
  ));
}
