import { Toast } from "@base-ui/react/toast";

/** Queues toasts from inside a ToastProvider (toast.tsx): show("List of citations copied.") puts one up. */
export function useToasts() {
  const manager = Toast.useToastManager();
  return { show: (title: string) => manager.add({ title }) };
}
