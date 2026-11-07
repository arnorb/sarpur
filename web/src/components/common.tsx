import { type ReactNode, useEffect, useRef } from "react";
import { Check, Download, Library, Plus, RotateCcw } from "lucide-react";
import type { DownloadMode, Show } from "@/api/types";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { isActive, useStore } from "@/store";

/** A native modal <dialog>: Escape and a click on the backdrop close it, and
 * focus returns to whatever opened it. */
export function ModalDialog({
  onClose,
  className,
  labelledBy,
  children,
}: {
  onClose: () => void;
  className: string;
  labelledBy: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog?.showModal();
    return () => {
      dialog?.close();
      if (trigger?.isConnected) trigger.focus();
    };
  }, []);
  return (
    <dialog
      ref={ref}
      aria-labelledby={labelledBy}
      className={cn("m-auto p-0 text-slate-100 backdrop:bg-black/70 backdrop:backdrop-blur-sm", className)}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      {children}
    </dialog>
  );
}

export function SubtitleBadges({ show }: { show: Show }) {
  const is = show.subtitleLanguages.includes("is");
  const en = show.subtitleLanguages.includes("en");
  if (!is && !en && !show.englishSubtitled && !show.downloadState) return null;
  return (
    <div className="mt-1 flex flex-wrap gap-1">
      {show.downloadState === "library" ? (
        <span className="rounded bg-emerald-800 px-1.5 py-0.5 text-[11px] font-medium text-emerald-50">In library</span>
      ) : show.downloadState === "recorded" ? (
        <span className="rounded bg-emerald-900 px-1.5 py-0.5 text-[11px] font-medium text-emerald-100">Downloaded before</span>
      ) : null}
      {is ? <span className="rounded bg-slate-700 px-1.5 py-0.5 text-[11px] text-slate-100">IS subtitles</span> : null}
      {en ? <span className="rounded bg-sky-800 px-1.5 py-0.5 text-[11px] text-sky-50">EN subtitles</span> : null}
      {show.englishSubtitled ? (
        <span className="rounded bg-violet-800 px-1.5 py-0.5 text-[11px] text-violet-50">English video version</span>
      ) : null}
    </div>
  );
}

export function FollowButton({ sid, followed, label, compact = false }: { sid: string; followed: boolean; label: string; compact?: boolean }) {
  const { setFollowed } = useStore();
  return (
    <Button
      size="sm"
      aria-pressed={followed}
      aria-label={`${followed ? "Following" : "Follow"} ${label}`}
      onClick={() => void setFollowed(sid, !followed)}
      className={cn(
        compact && "w-full text-xs",
        followed
          ? "border border-sky-600 bg-sky-700 text-white hover:border-rose-600 hover:bg-rose-700"
          : "border border-slate-600 bg-slate-800 text-slate-100 hover:bg-slate-700",
      )}
    >
      {followed ? <Check className="mr-1.5 h-4 w-4" aria-hidden /> : <Plus className="mr-1.5 h-4 w-4" aria-hidden />}
      {followed ? "Following" : "Follow"}
    </Button>
  );
}

/** Download and Library buttons; the browser's main action comes first and
 * is the primary button. Shows progress while a job for the pid runs. */
export function EpisodeActions({ show, onChoose }: { show: Show; onChoose: (mode: DownloadMode) => void }) {
  const { mainAction, jobs, allowRedownload } = useStore();
  const job = jobs.find((j) => j.pid === show.pid && isActive(j));
  const order: DownloadMode[] = mainAction === "library" ? ["library", "web"] : ["web", "library"];
  return (
    <div className="flex flex-wrap items-center justify-end gap-2">
      {show.downloadState === "recorded" ? (
        <Button
          variant="ghost"
          size="sm"
          className="h-8 px-2 text-xs text-slate-300 hover:bg-slate-800 hover:text-white"
          title="Let follow runs download this episode again"
          onClick={() => void allowRedownload(show)}
        >
          <RotateCcw className="mr-1 h-3.5 w-3.5" aria-hidden />
          Allow re-download
        </Button>
      ) : null}
      {order.map((mode, index) => (
        <Button
          key={mode}
          size="sm"
          onClick={() => onChoose(mode)}
          aria-label={`${mode === "web" ? "Download" : "Library"}: ${show.displayTitle}`}
          className={cn(
            "relative h-8 overflow-hidden",
            index === 0
              ? "border border-sky-600 bg-sky-700 text-white hover:bg-sky-600"
              : "border border-slate-600 bg-slate-800 text-slate-100 hover:bg-slate-700",
          )}
        >
          {job && job.mode === mode ? (
            <span
              className="absolute bottom-0 left-0 h-1 bg-sky-200/60 transition-[width] duration-200"
              style={{ width: `${Math.max(4, job.progress)}%` }}
            />
          ) : null}
          {mode === "web" ? <Download className="mr-1.5 h-4 w-4" aria-hidden /> : <Library className="mr-1.5 h-4 w-4" aria-hidden />}
          {mode === "web" ? "Download" : "Library"}
        </Button>
      ))}
    </div>
  );
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block text-sm">
      <span className="mb-1 block font-medium text-slate-200">{label}</span>
      {children}
      {hint ? <span className="mt-1 block text-xs text-slate-400">{hint}</span> : null}
    </label>
  );
}

export const inputClass = "h-9 border-slate-600 bg-slate-800 text-slate-100 placeholder:text-slate-400";
export const secondaryButton = "border border-slate-600 bg-slate-800 text-slate-100 hover:bg-slate-700";
export const primaryButton = "border border-sky-600 bg-sky-700 text-white hover:bg-sky-600";
