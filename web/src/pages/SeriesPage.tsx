import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, Download, ExternalLink, Library } from "lucide-react";
import { useNavigate, useParams } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { EpisodeActions, FollowButton, SubtitleBadges } from "@/components/common";
import { type DownloadChoice, DownloadDialog } from "@/components/DownloadDialog";
import { CONTENT_LABEL, episodeDescription, episodeMeta, groupBySid, groupDescription, groupFollowed, groupPoster, groupTitle, groupType } from "@/lib/shows";
import { usePageTitle } from "@/lib/page";
import { count } from "@/lib/utils";
import { useStore } from "@/store";

export function SeriesPage() {
  const { sid = "" } = useParams<{ sid: string }>();
  const navigate = useNavigate();
  const { shows, showsLoading, mainAction } = useStore();
  const group = useMemo(() => groupBySid(shows.filter((s) => s.sid === sid))[0] ?? null, [shows, sid]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [choice, setChoice] = useState<DownloadChoice | null>(null);
  const [posterFailed, setPosterFailed] = useState(false);
  useEffect(() => setSelected(new Set()), [sid]);
  usePageTitle(group ? groupTitle(group) : "Series");

  const back = () => (window.history.length > 1 ? navigate(-1) : navigate("/"));

  if (!group) {
    return (
      <section className="space-y-4">
        <Button size="sm" variant="secondary" onClick={back}>
          <ArrowLeft className="mr-2 h-4 w-4" aria-hidden />
          Back
        </Button>
        <h1 tabIndex={-1} className="text-xl font-semibold text-slate-100 focus:outline-none">
          {showsLoading ? "Loading…" : "Not found"}
        </h1>
        {showsLoading ? null : <p className="text-sm text-slate-300">This series isn’t in the catalogue, or not in the current search results.</p>}
      </section>
    );
  }

  const title = groupTitle(group);
  const type = groupType(group);
  const poster = groupPoster(group);
  const description = groupDescription(group);
  const single = type === "movie_or_docu" ? group.shows[0] : null;
  const chosen = group.shows.filter((s) => selected.has(s.pid));
  const batchModes: DownloadChoice["mode"][] = mainAction === "library" ? ["library", "web"] : ["web", "library"];

  return (
    <section className="max-w-6xl space-y-4" aria-labelledby="series-title">
      <Button size="sm" variant="secondary" onClick={back}>
        <ArrowLeft className="mr-2 h-4 w-4" aria-hidden />
        Back
      </Button>

      <div className="grid gap-4 rounded-xl border border-slate-800 bg-slate-900/60 p-3 md:grid-cols-[220px_1fr] md:p-4">
        <div className="mx-auto w-full max-w-[220px]">
          <div className="relative aspect-2/3 overflow-hidden rounded-lg border border-slate-800 bg-slate-800">
            {poster && !posterFailed ? (
              <img src={poster} alt={`${title} poster`} className="h-full w-full object-cover" onError={() => setPosterFailed(true)} />
            ) : (
              <div className="flex h-full w-full items-center justify-center text-xs font-medium text-slate-300">No image</div>
            )}
          </div>
        </div>

        <div className="min-w-0">
          <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 id="series-title" lang="is" tabIndex={-1} className="wrap-anywhere hyphens-auto text-xl font-semibold text-slate-100 focus:outline-none">
                {title}
              </h1>
              <p className="text-sm text-slate-300">
                {CONTENT_LABEL[type]} · {count(group.shows.length, "episode", "episodes")} ·{" "}
                <a
                  href={group.shows[0].webUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-1 rounded text-sky-200 underline hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400"
                >
                  On RÚV <ExternalLink className="h-3 w-3" aria-hidden />
                </a>
              </p>
            </div>
            <FollowButton sid={group.sid} followed={groupFollowed(group)} label={title} />
          </div>

          {single ? (
            <div className="mb-4 space-y-3">
              <p className="text-sm text-slate-300">Aired {episodeMeta(single)}</p>
              <SubtitleBadges show={single} />
              {episodeDescription(single) ? (
                <p lang="is" className="max-w-prose hyphens-auto text-sm leading-relaxed text-slate-200">
                  {episodeDescription(single)}
                </p>
              ) : null}
              <EpisodeActions show={single} onChoose={(mode) => setChoice({ shows: [single], mode })} />
            </div>
          ) : description ? (
            <section className="mb-4 rounded-md border border-slate-800 bg-slate-900 p-3">
              <h2 className="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-300">Description</h2>
              <p lang="is" className="max-w-prose hyphens-auto text-sm leading-relaxed text-slate-200">
                {description}
              </p>
            </section>
          ) : null}

          {!single ? (
            <div>
              <div className="mb-3 flex flex-wrap items-center gap-2 rounded-md border border-slate-800 bg-slate-900 p-3">
                <span className="mr-auto text-sm tabular-nums text-slate-200" aria-live="polite">
                  {selected.size} of {group.shows.length} selected
                </span>
                <Button size="sm" variant="secondary" className="h-8" onClick={() => setSelected(new Set(group.shows.map((s) => s.pid)))}>
                  Select all
                </Button>
                <Button size="sm" variant="secondary" className="h-8" onClick={() => setSelected(new Set())}>
                  Clear selection
                </Button>
                {batchModes.map((mode, index) => (
                  <Button
                    key={mode}
                    size="sm"
                    disabled={chosen.length === 0}
                    variant={index === 0 ? "default" : "secondary"}
                    className="h-8"
                    onClick={() => setChoice({ shows: chosen, mode })}
                  >
                    {mode === "web" ? <Download className="mr-2 h-4 w-4" aria-hidden /> : <Library className="mr-2 h-4 w-4" aria-hidden />}
                    {mode === "web" ? "Download selected" : "Add selected to library"}
                  </Button>
                ))}
              </div>
              <ul className="space-y-2" aria-label="Episodes">
                {group.shows.map((show) => (
                  <li key={show.pid} className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-slate-800 bg-slate-900 p-3">
                    <input
                      type="checkbox"
                      aria-label={`Select ${show.displayTitle}`}
                      className="h-4 w-4 shrink-0 accent-sky-500"
                      checked={selected.has(show.pid)}
                      onChange={(e) =>
                        setSelected((current) => {
                          const next = new Set(current);
                          if (e.target.checked) next.add(show.pid);
                          else next.delete(show.pid);
                          return next;
                        })
                      }
                    />
                    <div className="min-w-0 flex-1 basis-40">
                      <p lang="is" className="wrap-anywhere hyphens-auto font-medium text-slate-100">
                        {show.displayTitle}
                      </p>
                      <p className="text-sm text-slate-300">{episodeMeta(show)}</p>
                      <SubtitleBadges show={show} />
                      {episodeDescription(show) ? (
                        <p lang="is" className="mt-1 max-w-prose hyphens-auto text-sm text-slate-200">
                          {episodeDescription(show)}
                        </p>
                      ) : null}
                    </div>
                    <EpisodeActions show={show} onChoose={(mode) => setChoice({ shows: [show], mode })} />
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </div>
      </div>

      {choice ? <DownloadDialog choice={choice} onClose={() => setChoice(null)} /> : null}
    </section>
  );
}
