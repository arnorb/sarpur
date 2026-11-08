import { type CSSProperties, useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { EpisodeActions, FollowButton, SubtitleBadges } from "@/components/common";
import { type DownloadChoice, DownloadDialog } from "@/components/DownloadDialog";
import { useBrowse } from "@/components/Layout";
import { savePosterColumns } from "@/lib/browser";
import {
  episodeMeta,
  filterAndSort,
  formatDuration,
  type Group,
  groupBySid,
  groupFollowed,
  groupPoster,
  groupTitle,
  type SortBy,
  type SubtitleFilter,
} from "@/lib/shows";
import { scrollBehavior, usePageTitle } from "@/lib/page";
import { count } from "@/lib/utils";
import { useStore } from "@/store";

const PAGE_SIZE = 100;
const focusRing = "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400";
const select = `h-8 rounded-md border border-slate-500 bg-slate-800 px-2 text-base text-slate-100 sm:text-sm ${focusRing}`;
const CONTENT_LABEL: Record<string, string> = { show: "Shows", movie_or_docu: "Movies", sport: "Sports" };

export function BrowsePage() {
  const { shows, showsLoading, query } = useStore();
  const { filters, setFilters, view, page, setPage, columns, setColumns } = useBrowse();
  const [choice, setChoice] = useState<DownloadChoice | null>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const categoriesRef = useRef<HTMLDetailsElement>(null);
  const heading = query ? `Search results for “${query}”` : (CONTENT_LABEL[filters.contentType] ?? "Catalogue");
  usePageTitle(heading);

  const groups = useMemo(() => groupBySid(shows), [shows]);
  const categories = useMemo(
    () => Array.from(new Set(shows.flatMap((s) => s.categories))).sort((a, b) => a.localeCompare(b, "is")),
    [shows],
  );
  const visible = useMemo(() => filterAndSort(groups, filters), [groups, filters]);
  const pages = Math.max(1, Math.ceil(visible.length / PAGE_SIZE));
  useEffect(() => {
    if (page > pages) setPage(pages);
  }, [page, pages, setPage]);
  const pageGroups = visible.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);
  const changePage = (next: number) => {
    setPage(next);
    window.scrollTo({ top: 0, behavior: scrollBehavior() });
    // Screen-reader and keyboard users continue at the new page's first item.
    resultsRef.current?.focus({ preventScroll: true });
  };
  const closeCategories = (refocus: boolean) => {
    const details = categoriesRef.current;
    if (!details?.open) return;
    details.open = false;
    if (refocus) details.querySelector("summary")?.focus();
  };

  return (
    <>
      <h1 tabIndex={-1} className="sr-only">
        {heading}
      </h1>
      {/* relative: the categories popup is anchored to this row, so it stays on screen at 320 px. */}
      <div className="relative mb-4 flex flex-wrap items-center gap-2 md:gap-3">
        <span className="text-xs font-medium uppercase tracking-wide text-slate-300">Filters</span>
        <details
          ref={categoriesRef}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              closeCategories(true);
            }
          }}
          onBlur={(e) => {
            if (!e.currentTarget.contains(e.relatedTarget as Node | null)) closeCategories(false);
          }}
        >
          <summary className={`flex h-8 cursor-pointer list-none items-center rounded-md border border-slate-500 bg-slate-800 px-3 text-sm text-slate-100 hover:bg-slate-700 ${focusRing}`}>
            {filters.categories.length === 0 ? "All categories" : count(filters.categories.length, "category", "categories")}
          </summary>
          <div className="absolute left-0 top-10 z-30 max-h-80 w-64 max-w-full overflow-y-auto overscroll-contain rounded-lg border border-slate-700 bg-slate-900 p-2 shadow-2xl">
            <div className="mb-1 flex items-center justify-between border-b border-slate-800 px-2 pb-2">
              <span className="text-xs font-semibold uppercase tracking-wide text-slate-300">Categories</span>
              {filters.categories.length > 0 ? (
                <button type="button" className={`rounded text-xs text-sky-200 hover:text-white ${focusRing}`} onClick={() => setFilters({ ...filters, categories: [] })}>
                  Clear
                </button>
              ) : null}
            </div>
            {categories.map((category) => (
              <label key={category} className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm text-slate-100 hover:bg-slate-800">
                <input
                  type="checkbox"
                  className="h-4 w-4 accent-sky-500"
                  checked={filters.categories.includes(category)}
                  onChange={(e) =>
                    setFilters({
                      ...filters,
                      categories: e.target.checked ? [...filters.categories, category] : filters.categories.filter((c) => c !== category),
                    })
                  }
                />
                <span lang="is">{category}</span>
              </label>
            ))}
          </div>
        </details>
        <select aria-label="Subtitles" className={select} value={filters.subtitles} onChange={(e) => setFilters({ ...filters, subtitles: e.target.value as SubtitleFilter })}>
          <option value="all">All subtitle options</option>
          <option value="is">Icelandic subtitles</option>
          <option value="en">English subtitle track</option>
          <option value="english-version">English-subtitled video</option>
        </select>
        <select aria-label="Sort by" className={select} value={filters.sortBy} onChange={(e) => setFilters({ ...filters, sortBy: e.target.value as SortBy })}>
          <option value="name">Name</option>
          <option value="duration">Duration</option>
          <option value="firstAppearance">First aired</option>
          <option value="lastAppearance">Last aired</option>
          <option value="expiry">Expiry date</option>
        </select>
        <select aria-label="Sort direction" className={select} value={filters.direction} onChange={(e) => setFilters({ ...filters, direction: e.target.value as "asc" | "desc" })}>
          <option value="asc">Ascending</option>
          <option value="desc">Descending</option>
        </select>
        {filters.categories.length > 0 || filters.subtitles !== "all" ? (
          <button type="button" className={`rounded text-xs text-sky-200 hover:text-white ${focusRing}`} onClick={() => setFilters({ ...filters, categories: [], subtitles: "all" })}>
            Clear filters
          </button>
        ) : null}
        {view === "poster" ? (
          <label className="ml-auto hidden items-center gap-2 text-xs text-slate-300 lg:flex">
            Poster columns
            <input
              type="range"
              min={5}
              max={15}
              value={columns}
              className="w-36 accent-sky-500"
              onChange={(e) => {
                setColumns(Number(e.target.value));
                savePosterColumns(Number(e.target.value));
              }}
            />
            <span className="w-5 text-right font-semibold tabular-nums text-slate-100">{columns}</span>
          </label>
        ) : null}
      </div>

      <div ref={resultsRef} tabIndex={-1} aria-label={`${heading}, page ${page}`} className="focus:outline-none">
      {showsLoading && shows.length === 0 ? (
        <p className="text-sm text-slate-300">Loading…</p>
      ) : visible.length === 0 ? (
        <p className="text-sm text-slate-300">{query ? "Nothing matches the search." : shows.length === 0 ? "The catalogue is empty." : "Nothing matches the filters."}</p>
      ) : view === "poster" ? (
        <ul className="poster-grid gap-4" style={{ "--poster-columns": columns } as CSSProperties} aria-label="Series">
          {pageGroups.map((group) => (
            <PosterCard key={group.sid} group={group} />
          ))}
        </ul>
      ) : (
        <div className="space-y-6 rounded-lg border border-slate-800 bg-slate-900/60 p-3 md:p-4">
          {pageGroups.map((group) => (
            <ListGroup key={group.sid} group={group} onChoose={(show, mode) => setChoice({ shows: [show], mode })} />
          ))}
        </div>
      )}
      </div>

      {visible.length > 0 ? (
        <nav aria-label="Pages" className="mt-6 flex flex-wrap items-center justify-between gap-3">
          <p className="text-sm tabular-nums text-slate-300">
            Showing {(page - 1) * PAGE_SIZE + 1}–{Math.min(page * PAGE_SIZE, visible.length)} of {visible.length}
          </p>
          <div className="flex items-center gap-2">
            <Button size="sm" variant="secondary" disabled={page === 1} onClick={() => changePage(page - 1)}>
              Previous
            </Button>
            <span className="text-sm tabular-nums text-slate-300">
              {page} / {pages}
            </span>
            <Button size="sm" variant="secondary" disabled={page === pages} onClick={() => changePage(page + 1)}>
              Next
            </Button>
          </div>
        </nav>
      ) : null}

      {choice ? <DownloadDialog choice={choice} onClose={() => setChoice(null)} /> : null}
    </>
  );
}

function PosterCard({ group }: { group: Group }) {
  const [failed, setFailed] = useState(false);
  const title = groupTitle(group);
  const poster = groupPoster(group);
  const followed = groupFollowed(group);
  const duration = group.shows.length === 1 ? formatDuration(group.shows[0]) : null;
  const href = `/title/${encodeURIComponent(group.sid)}`;
  return (
    <li className="group motion-safe:transition-transform motion-safe:hover:-translate-y-0.5">
      <div className="relative aspect-2/3 overflow-hidden rounded-lg bg-slate-800">
        <Link to={href} tabIndex={-1} aria-hidden="true" className="block h-full w-full">
          {poster && !failed ? (
            <img
              src={poster}
              alt=""
              loading="lazy"
              onError={() => setFailed(true)}
              className="h-full w-full object-cover motion-safe:transition-transform motion-safe:duration-300 motion-safe:group-hover:scale-105"
            />
          ) : (
            <div className="flex h-full w-full items-center justify-center p-2 text-center text-xs font-medium text-slate-300">No image</div>
          )}
        </Link>
        {/* The follow bar: on hover or keyboard focus, always when followed. */}
        <div
          className={`absolute inset-x-0 bottom-0 bg-linear-to-t from-black/90 to-transparent px-2 pb-2 pt-6 transition-opacity duration-200 ${
            followed ? "opacity-100" : "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 pointer-coarse:opacity-100"
          }`}
        >
          <FollowButton sid={group.sid} followed={followed} label={title} compact />
        </div>
      </div>
      <div className="pt-2">
        <Link lang="is" to={href} className="block wrap-anywhere hyphens-auto rounded text-sm font-semibold text-slate-100 hover:text-sky-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400">
          {title}
        </Link>
        {duration ? <p className="mt-0.5 text-xs text-slate-300">{duration}</p> : null}
      </div>
    </li>
  );
}

function ListGroup({ group, onChoose }: { group: Group; onChoose: DownloadChoiceHandler }) {
  const title = groupTitle(group);
  return (
    <section aria-label={title}>
      <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <Link
            lang="is"
            to={`/title/${encodeURIComponent(group.sid)}`}
            className={`wrap-anywhere hyphens-auto rounded text-base font-semibold text-slate-100 hover:text-sky-200 ${focusRing}`}
          >
            {title}
          </Link>
          <p className="text-xs text-slate-300">{count(group.shows.length, "episode", "episodes")}</p>
        </div>
        <FollowButton sid={group.sid} followed={groupFollowed(group)} label={title} />
      </div>
      <ul className="space-y-2">
        {group.shows.map((show) => (
          <li key={show.pid} className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-slate-800 bg-slate-900 p-3">
            <div className="min-w-0 flex-1 basis-48">
              <p lang="is" className="wrap-anywhere hyphens-auto text-sm font-medium text-slate-100">
                {show.displayTitle}
              </p>
              <p className="text-xs text-slate-300">{episodeMeta(show)}</p>
              <SubtitleBadges show={show} />
            </div>
            <EpisodeActions show={show} onChoose={(mode) => onChoose(show, mode)} />
          </li>
        ))}
      </ul>
    </section>
  );
}

type DownloadChoiceHandler = (show: Group["shows"][number], mode: DownloadChoice["mode"]) => void;
