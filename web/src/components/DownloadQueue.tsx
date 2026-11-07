import { useState } from "react";
import { Download, X } from "lucide-react";
import type { Job } from "@/api/types";
import { Button } from "@/components/ui/button";
import { isActive, isFinished, useStore } from "@/store";

const STATE_LABEL: Record<Job["state"], string> = {
  queued: "Waiting",
  running: "Downloading",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
};
const STATE_CLASS: Record<Job["state"], string> = {
  queued: "bg-amber-500/20 text-amber-200",
  running: "bg-sky-500/20 text-sky-200",
  completed: "bg-emerald-500/20 text-emerald-200",
  failed: "bg-rose-500/20 text-rose-200",
  cancelled: "bg-slate-700 text-slate-200",
};

export function DownloadQueue() {
  const { jobs, removeJob, retryJob, clearFinished } = useStore();
  const [open, setOpen] = useState(true);
  if (jobs.length === 0) return null;
  const active = jobs.filter(isActive).length;
  const done = jobs.filter(isFinished).length;

  if (!open) {
    return (
      <Button className="fixed bottom-4 right-4 z-40 bg-sky-700 text-white shadow-xl hover:bg-sky-600" onClick={() => setOpen(true)}>
        <Download className="mr-2 h-4 w-4" aria-hidden />
        Downloads ({active || jobs.length})
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
          <h2 className="text-sm font-semibold text-slate-100">Download queue</h2>
          <p className="text-xs text-slate-300">
            {active} active or waiting · {done} finished
          </p>
        </div>
        <div className="flex items-center gap-1">
          {done > 0 ? (
            <Button variant="ghost" size="sm" className="h-8 px-2 text-xs text-slate-300 hover:bg-slate-800 hover:text-white" onClick={() => void clearFinished()}>
              Clear finished
            </Button>
          ) : null}
          <Button
            variant="ghost"
            size="sm"
            aria-label="Close the download queue"
            title="Close; downloads continue in the background"
            className="h-8 w-8 p-0 text-slate-300 hover:bg-slate-800 hover:text-white"
            onClick={() => setOpen(false)}
          >
            <X className="h-4 w-4" aria-hidden />
          </Button>
        </div>
      </div>
      <ul className="max-h-72 space-y-2 overflow-y-auto p-3">
        {jobs.slice(0, 20).map((job) => (
          <li key={job.id} className="rounded-lg border border-slate-800 bg-slate-950/70 p-3">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium text-slate-100" title={job.title}>
                  {job.title}
                </p>
                <div className="mt-1 flex flex-wrap items-center gap-1.5">
                  <span className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${STATE_CLASS[job.state]}`}>{STATE_LABEL[job.state]}</span>
                  <span className="text-xs text-slate-300">
                    {job.mode === "library" ? "Library" : "This computer"}
                    {job.priority === "scheduled" ? " · followed" : ""} · {Math.round(job.progress)}%
                  </span>
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                {job.state === "failed" || job.state === "cancelled" ? (
                  <Button variant="outline" size="sm" className="h-7 border-sky-700 bg-sky-950 px-2 text-xs text-sky-100 hover:bg-sky-900" onClick={() => void retryJob(job)}>
                    Retry
                  </Button>
                ) : null}
                {isActive(job) ? (
                  <Button variant="outline" size="sm" className="h-7 border-rose-700 bg-rose-950 px-2 text-xs text-rose-100 hover:bg-rose-900" onClick={() => void removeJob(job)}>
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
              <div
                className={`h-full transition-[width] duration-300 ${job.state === "failed" ? "bg-rose-500" : job.state === "cancelled" ? "bg-slate-500" : "bg-sky-500"}`}
                style={{ width: `${job.progress}%` }}
              />
            </div>
            {job.mode === "web" && isActive(job) ? <p className="mt-1 text-xs text-slate-300">Keep this tab open; the file is saved when the download finishes.</p> : null}
            {job.state === "running" && job.message && job.message !== "Downloading" ? <p className="mt-1 text-xs text-slate-300">{job.message}</p> : null}
            {job.state === "failed" ? <p className="mt-1 break-words text-xs text-rose-200">{job.error || job.message || "The download failed. You can retry it."}</p> : null}
            {job.state === "cancelled" ? <p className="mt-1 text-xs text-slate-300">Stopped before it finished.</p> : null}
            {job.state === "completed" ? (
              <p className="mt-1 text-xs text-emerald-200">
                {job.message === "Already in the library" ? "Already in the library." : "Done."}
                {job.mode === "web"
                  ? job.files.map((file) => (
                      <a key={file.url} href={file.url} download={file.name} className="ml-2 text-sky-200 underline hover:text-white">
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
