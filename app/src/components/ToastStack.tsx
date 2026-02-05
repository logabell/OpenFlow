import { useEffect } from "react";
import { useAppStore } from "../state/appStore";

type Variant = "info" | "success" | "warning" | "error";

const VARIANT_STYLES: Record<Variant, string> = {
  info: "bg-slate-900/90 border-slate-700 text-slate-100",
  success: "bg-emerald-900/90 border-emerald-600 text-emerald-100",
  warning: "bg-amber-900/90 border-amber-600 text-amber-100",
  error: "bg-rose-900/90 border-rose-600 text-rose-100",
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
            className={`pointer-events-auto w-full max-w-sm rounded-xl border px-4 py-3 shadow-lg ${styles}`}
          >
            <div className="flex items-start justify-between gap-3">
              <div>
                <p className="text-sm font-semibold leading-tight">{toast.title}</p>
                {toast.description && (
                  <p className="mt-1 text-xs text-white/80">{toast.description}</p>
                )}
              </div>
              <button
                type="button"
                className="text-xs uppercase text-white/70 hover:text-white"
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
