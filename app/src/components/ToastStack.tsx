import { useEffect } from "react";
import { useAppStore } from "../state/appStore";

type Variant = "info" | "success" | "warning" | "error";

const VARIANT_STYLES: Record<Variant, string> = {
  info: "bg-surface border-border text-fg",
  success: "bg-good/10 border-good/35 text-fg",
  warning: "bg-warn/10 border-warn/40 text-fg",
  error: "bg-bad/10 border-bad/35 text-fg",
};

const ToastStack = () => {
  const { toasts, dismissToast } = useAppStore((state) => ({
    toasts: state.toasts,
    dismissToast: state.dismissToast,
  }));

  useEffect(() => {
    const timers = toasts.map((toast) =>
      window.setTimeout(() => dismissToast(toast.id), 6000),
    );
    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, [toasts, dismissToast]);

  if (toasts.length === 0) {
    return null;
  }

  return (
    <div className="pointer-events-none fixed inset-x-0 top-4 z-[1000] flex flex-col items-center gap-3 px-4">
      {toasts.map((toast) => {
        const variant: Variant = toast.variant ?? "info";
        const styles = VARIANT_STYLES[variant] ?? VARIANT_STYLES.info;
        return (
          <div
            key={toast.id}
            className={`pointer-events-auto w-full max-w-sm rounded-vibe border px-4 py-3 shadow-[0_6px_0_hsl(var(--shadow)/0.22),0_18px_50px_hsl(var(--shadow)/0.35)] ${styles}`}
          >
            <div className="flex items-start justify-between gap-3">
              <div>
                <p className="text-sm font-semibold leading-tight">{toast.title}</p>
                {toast.description && (
                  <p className="mt-1 text-xs text-muted">{toast.description}</p>
                )}
              </div>
              <button
                type="button"
                className="text-xs uppercase text-muted hover:text-fg"
                onClick={() => dismissToast(toast.id)}
              >
                Close
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
};

export default ToastStack;
