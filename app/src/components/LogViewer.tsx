import { useAppStore } from "../state/appStore";

const LogViewer = () => {
  const { logs, toggleLogViewer } = useAppStore((state) => ({
    logs: state.logs,
    toggleLogViewer: state.toggleLogViewer,
  }));

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-6">
      <div className="flex h-[70vh] w-[720px] max-w-full flex-col overflow-hidden rounded-2xl bg-[#0f172a] shadow-2xl">
        <header className="flex items-center justify-between border-b border-white/10 px-4 py-3">
          <h2 className="text-lg font-semibold text-white">Logs</h2>
          <button
            type="button"
            className="rounded-full bg-white/10 px-3 py-1 text-xs uppercase text-white hover:bg-white/20"
            onClick={() => toggleLogViewer(false)}
          >
            Close
          </button>
        </header>
        <pre className="flex-1 overflow-y-auto bg-slate-950 px-4 py-3 text-xs text-slate-100">
          {logs.length === 0 ? "No log data yet." : logs.join("\n")}
        </pre>
      </div>
    </div>
  );
};

export default LogViewer;
