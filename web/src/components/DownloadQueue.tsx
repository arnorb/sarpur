import { useEffect, useRef, useState } from "react";
import { Download, X } from "lucide-react";
import type { Job } from "@/api/types";
import { Button } from "@/components/ui/button";
import { isActive, useStore } from "@/store";

const STATE_LABEL: Record<Job["state"], string> = {
  queued: "Waiting",
  running: "Downloading",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
};
const STATE_CLASS: Record<Job["state"], string> = {
  queued: "bg-slate-700 text-slate-200",
  running: "bg-sky-500/20 text-sky-200",
  completed: "bg-emerald-500/20 text-emerald-200",
  failed: "bg-rose-500/20 text-rose-200",
  cancelled: "bg-slate-700 text-slate-200",
};
const BAR_CLASS: Record<Job["state"], string> = {
  queued: "bg-slate-500",
  running: "bg-sky-500",
  completed: "bg-emerald-500",
  failed: "bg-rose-500",
  cancelled: "bg-slate-500",
};

const OPEN_KEY = "sarpur.queueOpen";
function loadOpen(): boolean {
  try {
    return localStorage.getItem(OPEN_KEY) === "1";
  } catch {
    return false;
  }
}

export function DownloadQueue() {
  const { jobs, removeJob, retryJob, clearFinished } = useStore();
  // Collapsed unless this browser opened it last time: the open panel covers
  // the bottom right of the page (the episode buttons, Next).
  const [open, setOpenState] = useState(loadOpen);
  const toggled = useRef(false);
  const toggleRef = useRef<HTMLButtonElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);

  const setOpen = (value: boolean) => {
    toggled.current = true;
    setOpenState(value);
    try {
      localStorage.setItem(OPEN_KEY, value ? "1" : "0");
    } catch {
      // Not remembered; fine.
    }
  };
  // Keep keyboard focus on the control that replaces the one just pressed.
  useEffect(() => {
    if (!toggled.current) return;
    toggled.current = false;
    (open ? closeRef : toggleRef).current?.focus();
  }, [open]);

  if (jobs.length === 0) return null;
  const active = jobs.filter(isActive).length;
  const completed = jobs.filter((job) => job.state === "completed").length;

  if (!open) {
    return (
      <Button ref={toggleRef} aria-expanded={false} className="fixed bottom-4 right-4 z-40 shadow-xl" onClick={() => setOpen(true)}>
        <Download className="mr-2 h-4 w-4" aria-hidden />
        {active > 0 ? <span className="tabular-nums">Downloads ({active} active)</span> : "Downloads"}
      </Button>
    );
  }
  return (
    <aside
      aria-label="Download queue"
      className="fixed bottom-4 right-4 z-40 w-[min(24rem,calc(100vw-2rem))] overflow-hidden rounded-xl border border-slate-700 bg-slate-900 shadow-2xl"
    >
      <div className="flex items-center justify-between gap-3 border-b border-slate-800 px-4 py-3">
        <div>
          <h2 ref={headingRef} tabIndex={-1} className="rounded text-sm font-semibold text-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400">
            Download queue
          </h2>
          <p className="text-xs tabular-nums text-slate-300">
            {active} active or waiting · {jobs.length - active} finished
          </p>
        </div>
        <div className="flex items-center gap-1">
          {completed > 0 ? (
            <Button
              variant="ghost"
              size="sm"
              className="h-8 px-2 text-xs"
              onClick={async () => {
                headingRef.current?.focus();
                await clearFinished();
              }}
            >
              Clear completed
            </Button>
          ) : null}
          <Button
            ref={closeRef}
            variant="ghost"
            size="sm"
            aria-expanded
            aria-label="Close the download queue"
            title="Close; downloads continue in the background"
            className="h-8 w-8 p-0"
            onClick={() => setOpen(false)}
          >
            <X className="h-4 w-4" aria-hidden />
          </Button>
        </div>
      </div>
      <ul className="max-h-72 space-y-2 overflow-y-auto overscroll-contain p-3">
        {jobs.map((job) => (
          <li key={job.id} className="rounded-lg border border-slate-800 bg-slate-950/70 p-3">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 flex-1">
                <p lang="is" className="wrap-anywhere hyphens-auto text-sm font-medium text-slate-100">
                  {job.title}
                </p>
                <div className="mt-1 flex flex-wrap items-center gap-1.5">
                  <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${STATE_CLASS[job.state]}`}>{STATE_LABEL[job.state]}</span>
                  <span className="text-xs tabular-nums text-slate-300">
                    {job.mode === "library" ? "Library" : "This computer"}
                    {job.priority === "scheduled" ? " · followed" : ""} · {Math.round(job.progress)}%
                  </span>
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                {job.state === "failed" || job.state === "cancelled" ? (
                  <Button variant="secondary" size="sm" className="h-7 px-2 text-xs" onClick={() => void retryJob(job)}>
                    Retry
                  </Button>
                ) : null}
                {isActive(job) ? (
                  <Button
                    variant="secondary"
                    size="sm"
                    className="h-7 border-rose-700 bg-rose-950 px-2 text-xs text-rose-100 hover:bg-rose-900"
                    onClick={() => void removeJob(job)}
                  >
                    Stop
                  </Button>
                ) : null}
              </div>
            </div>
            <div
              className="mt-2 h-1.5 overflow-hidden rounded-full bg-slate-800"
              role="progressbar"
              aria-label={`${job.title} progress`}
              aria-valuenow={Math.round(job.progress)}
              aria-valuemin={0}
              aria-valuemax={100}
            >
              <div className={`h-full transition-[width] duration-300 ${BAR_CLASS[job.state]}`} style={{ width: `${job.progress}%` }} />
            </div>
            {job.mode === "web" && isActive(job) ? <p className="mt-1 text-xs text-slate-300">Keep this tab open; the file is saved when the download finishes.</p> : null}
            {job.state === "running" && job.message && job.message !== "Downloading" ? <p className="mt-1 text-xs text-slate-300">{job.message}</p> : null}
            {job.state === "failed" ? (
              <>
                <p className="mt-1 text-xs text-rose-200">The download failed. Retry, or check that the episode is still on RÚV.</p>
                {job.error || job.message ? <p className="mt-0.5 break-words text-xs text-slate-400">{job.error || job.message}</p> : null}
              </>
            ) : null}
            {job.state === "cancelled" ? <p className="mt-1 text-xs text-slate-300">Stopped before it finished.</p> : null}
            {job.state === "completed" ? (
              <p className="mt-1 text-xs text-emerald-200">
                {job.message === "Already in the library" ? "Already in the library." : "Done."}
                {job.mode === "web"
                  ? job.files.map((file) => (
                      <a key={file.url} href={file.url} download={file.name} className="ml-2 rounded text-sky-200 underline hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400">
                        Save again{job.files.length > 1 ? `: ${file.name}` : ""}
                      </a>
                    ))
                  : null}
              </p>
            ) : null}
          </li>
        ))}
      </ul>
    </aside>
  );
}
