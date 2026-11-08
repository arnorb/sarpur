import { cloneElement, isValidElement, type ReactElement, type ReactNode, useEffect, useId, useRef, useState } from "react";
import { Check, Download, Library, Plus, RotateCcw, X } from "lucide-react";
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
        <span className="rounded bg-emerald-800 px-1.5 py-0.5 text-xs font-medium text-emerald-50">In library</span>
      ) : show.downloadState === "recorded" ? (
        <span className="rounded bg-emerald-900 px-1.5 py-0.5 text-xs font-medium text-emerald-100">Downloaded before</span>
      ) : null}
      {is ? <span className="rounded bg-slate-700 px-1.5 py-0.5 text-xs text-slate-100">IS subtitles</span> : null}
      {en ? <span className="rounded bg-slate-700 px-1.5 py-0.5 text-xs text-slate-100">EN subtitles</span> : null}
      {show.englishSubtitled ? (
        <span className="rounded border border-slate-500 px-1.5 py-0.5 text-xs text-slate-100">English video version</span>
      ) : null}
    </div>
  );
}

export function FollowButton({ sid, followed, label, compact = false }: { sid: string; followed: boolean; label: string; compact?: boolean }) {
  const { setFollowed } = useStore();
  return (
    // The name stays "Follow …" with aria-pressed; on hover and keyboard focus
    // a followed button says what a click does: Unfollow.
    <Button
      size="sm"
      variant={followed ? "default" : "secondary"}
      aria-pressed={followed}
      aria-label={`Follow ${label}`}
      onClick={() => void setFollowed(sid, !followed)}
      className={cn("group/follow", compact && "w-full text-xs")}
    >
      {followed ? (
        <>
          <span className="inline-flex items-center group-hover/follow:hidden group-focus-visible/follow:hidden">
            <Check className="mr-1.5 h-4 w-4" aria-hidden />
            Following
          </span>
          <span className="hidden items-center group-hover/follow:inline-flex group-focus-visible/follow:inline-flex">
            <X className="mr-1.5 h-4 w-4" aria-hidden />
            Unfollow
          </span>
        </>
      ) : (
        <>
          <Plus className="mr-1.5 h-4 w-4" aria-hidden />
          Follow
        </>
      )}
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
        <Button variant="ghost" size="sm" className="h-8 px-2 text-xs" onClick={() => void allowRedownload(show)}>
          <RotateCcw className="mr-1 h-3.5 w-3.5" aria-hidden />
          Include in follow runs
        </Button>
      ) : null}
      {order.map((mode, index) => (
        <Button
          key={mode}
          size="sm"
          onClick={() => onChoose(mode)}
          aria-label={`${mode === "web" ? "Download" : "Library"}: ${show.displayTitle}`}
          variant={index === 0 ? "default" : "secondary"}
          className="relative h-8 overflow-hidden"
        >
          {job && job.mode === mode ? (
            <span
              className="absolute bottom-0 left-0 h-1 bg-sky-200 transition-[width] duration-200"
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

/** A labelled form control; the hint is its description, not part of its name. */
export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  const hintId = useId();
  const control =
    hint && isValidElement(children) ? cloneElement(children as ReactElement<{ "aria-describedby"?: string }>, { "aria-describedby": hintId }) : children;
  return (
    <div className="text-sm">
      <label className="block">
        <span className="mb-1 block font-medium text-slate-200">{label}</span>
        {control}
      </label>
      {hint ? (
        <p id={hintId} className="mt-1 text-xs text-slate-400">
          {hint}
        </p>
      ) : null}
    </div>
  );
}

/** Screen-reader announcements of the current notice. Always mounted, so a new
 * text is announced (a region inserted together with its text often isn't). */
export function LiveNotice() {
  const { notice } = useStore();
  return (
    <>
      <div role="status" className="sr-only">
        {notice?.kind === "status" ? notice.text : ""}
      </div>
      <div role="alert" className="sr-only">
        {notice?.kind === "error" ? notice.text : ""}
      </div>
    </>
  );
}

/** The current notice, with Undo when the action can be undone. */
export function NoticeBar({ onDismiss, className }: { onDismiss?: () => void; className?: string }) {
  const { notice, clearNotice, fail } = useStore();
  const [undoing, setUndoing] = useState(false);
  if (!notice) return null;
  const undo = notice.undo;
  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md border px-3 py-2 text-sm",
        notice.kind === "error" ? "border-rose-500/50 bg-rose-950 text-rose-100" : "border-emerald-500/50 bg-emerald-950 text-emerald-100",
        className,
      )}
    >
      <span className="min-w-0 flex-1 break-words">{notice.text}</span>
      {undo ? (
        <Button
          variant="secondary"
          size="sm"
          className="h-7 px-2 text-xs"
          aria-disabled={undoing}
          onClick={async () => {
            if (undoing) return;
            setUndoing(true);
            try {
              await undo();
              clearNotice();
            } catch (error) {
              fail(error, "Couldn't undo");
            } finally {
              setUndoing(false);
            }
          }}
        >
          Undo
        </Button>
      ) : null}
      <button
        type="button"
        aria-label="Dismiss"
        className="rounded p-0.5 hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400"
        onClick={() => {
          clearNotice();
          onDismiss?.();
        }}
      >
        <X className="h-4 w-4" aria-hidden />
      </button>
    </div>
  );
}
