import { Link } from "react-router-dom";
import { Play, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useBrowse } from "@/components/Layout";
import { usePageTitle } from "@/lib/page";
import { count } from "@/lib/utils";
import { formatDate } from "@/lib/shows";
import { useStore } from "@/store";

export function FollowingPage() {
  const { follows, runStatus, settings, runFollows, removeFollowSid, unfollow } = useStore();
  const report = runStatus?.lastReport;
  const { openSettings } = useBrowse();
  usePageTitle("Following");
  const running = !!runStatus?.running;

  return (
    <section className="max-w-4xl space-y-6" aria-labelledby="following-title">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 id="following-title" tabIndex={-1} className="text-xl font-semibold text-slate-100 focus:outline-none">
            Following
          </h1>
          <p className="text-sm text-slate-300">
            New episodes of these shows go to the library
            {settings?.autoEnabled ? (
              ` every ${settings.autoIntervalMinutes} minutes`
            ) : (
              <>
                {" "}
                when{" "}
                <button
                  type="button"
                  className="rounded text-sky-200 underline hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400"
                  onClick={() => openSettings("Automatic downloads")}
                >
                  automatic downloads
                </button>{" "}
                are on
              </>
            )}
            . New seasons are added automatically when RÚV gives them a new series id.
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="secondary" aria-disabled={running} onClick={() => !running && void runFollows(true)}>
            Preview
          </Button>
          <Button size="sm" aria-disabled={running} onClick={() => !running && void runFollows(false)}>
            <Play className="mr-2 h-4 w-4" aria-hidden />
            {runStatus?.running ? "Running…" : "Run now"}
          </Button>
        </div>
      </div>

      {runStatus?.lastRunAt ? (
        <div
          role="status"
          className={`rounded-lg border p-3 text-sm ${
            runStatus.lastRunState === "ok"
              ? "border-slate-700 bg-slate-900 text-slate-200"
              : runStatus.lastRunState === "warning"
                ? "border-amber-500/50 bg-amber-950 text-amber-100"
                : "border-rose-500/50 bg-rose-950 text-rose-100"
          }`}
        >
          <p>
            Last run {formatDate(runStatus.lastRunAt)}
            {report?.dryRun ? " (preview)" : ""}: {runStatus.running ? "running…" : runStatus.lastRunMessage}
          </p>
          {report && !runStatus.running ? (
            <>
              <p className="mt-1 text-xs opacity-90">
                Skipped: {report.skipped.onDisk} in the library, {report.skipped.nameMatch} by file name, {report.skipped.recorded} downloaded before,{" "}
                {report.skipped.inQueue} already queued.
              </p>
              {report.addedSids.length > 0 ? (
                <p className="mt-1 text-xs">{report.dryRun ? "Would add" : "Added"}: {report.addedSids.map((a) => `${a.seriesTitle} (${a.sid})`).join(", ")}</p>
              ) : null}
              {report.scheduled.length > 0 ? (
                <details className="mt-2">
                  <summary className="cursor-pointer rounded text-xs underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400">
                    {report.dryRun ? "Would download" : "Queued"} {count(report.scheduled.length, "episode", "episodes")}
                  </summary>
                  <ul className="mt-1 max-h-48 overflow-y-auto overscroll-contain text-xs">
                    {report.scheduled.map((e) => (
                      <li key={e.pid} className="break-all py-0.5">
                        {e.title} → <code>{e.path}</code>
                      </li>
                    ))}
                  </ul>
                </details>
              ) : null}
              {report.warnings.length > 0 ? (
                <ul className="mt-2 list-inside list-disc text-xs">
                  {report.warnings.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              ) : null}
            </>
          ) : null}
        </div>
      ) : null}

      {follows.length === 0 ? (
        <p className="text-sm text-slate-300">You don’t follow anything yet. Use Follow on a show’s poster or page.</p>
      ) : (
        <ul className="space-y-3">
          {follows.map((follow) => (
            <li key={follow.id} className="rounded-lg border border-slate-800 bg-slate-900 p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h2 className="font-semibold text-slate-100">
                  <span lang="is" className="wrap-anywhere hyphens-auto">
                    {follow.label}
                  </span>
                  {follow.englishSubtitled ? (
                    <span className="ml-2 rounded border border-slate-500 px-1.5 py-0.5 text-xs font-medium text-slate-100">English video version</span>
                  ) : null}
                </h2>
                <Button size="sm" variant="secondary" className="h-8 border-rose-700 bg-rose-950 text-rose-100 hover:bg-rose-900" onClick={() => void unfollow(follow)}>
                  <Trash2 className="mr-1.5 h-4 w-4" aria-hidden />
                  Unfollow
                </Button>
              </div>
              <ul className="mt-2 space-y-1">
                {follow.sids.map((s) => (
                  <li key={s.sid} className="flex flex-wrap items-center justify-between gap-2 rounded-md bg-slate-950/60 px-3 py-2 text-sm">
                    <span className="min-w-0">
                      <Link
                        lang="is"
                        to={`/title/${encodeURIComponent(s.sid)}`}
                        className="rounded text-slate-100 underline-offset-2 hover:text-sky-200 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400"
                      >
                        {s.seriesTitle}
                      </Link>{" "}
                      <span className="text-xs text-slate-300">({s.sid})</span>
                      {s.addedBy === "auto" ? (
                        <span className="ml-2 rounded bg-slate-700 px-1.5 py-0.5 text-xs text-slate-100">added automatically</span>
                      ) : null}
                    </span>
                    <Button
                      variant="ghost"
                      size="sm"
                      aria-label={`Stop following ${s.seriesTitle} (${s.sid})`}
                      className="h-7 px-2 text-xs"
                      onClick={() => void removeFollowSid(follow, s.sid)}
                    >
                      <X className="mr-1 h-3.5 w-3.5" aria-hidden />
                      Remove
                    </Button>
                  </li>
                ))}
              </ul>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
