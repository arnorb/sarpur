import { createContext, type FormEvent, useContext, useEffect, useRef, useState } from "react";
import { Clapperboard, Film, Heart, RefreshCw, Search, Settings as SettingsIcon, Tv, Volleyball, XCircle } from "lucide-react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import type { ContentType } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { DownloadQueue } from "@/components/DownloadQueue";
import { LiveNotice, NoticeBar } from "@/components/common";
import { type Section, SettingsDialog } from "@/components/SettingsDialog";
import { loadPosterColumns } from "@/lib/browser";
import { DEFAULT_FILTERS, type Filters } from "@/lib/shows";
import { cn } from "@/lib/utils";
import { useStore } from "@/store";

/** Browse state that survives a visit to a series page. */
export type BrowseState = {
  filters: Filters;
  setFilters: (f: Filters) => void;
  view: "poster" | "list";
  setView: (v: "poster" | "list") => void;
  page: number;
  setPage: (p: number) => void;
  columns: number;
  setColumns: (c: number) => void;
  /** Opens Settings, at a section if given. */
  openSettings: (section?: Section) => void;
};

const BrowseContext = createContext<BrowseState | null>(null);
export function useBrowse(): BrowseState {
  const state = useContext(BrowseContext);
  if (!state) throw new Error("useBrowse outside Layout");
  return state;
}

const NAV: { key: ContentType; label: string; Icon: typeof Tv }[] = [
  { key: "show", label: "Shows", Icon: Tv },
  { key: "movie_or_docu", label: "Movies", Icon: Film },
  { key: "sport", label: "Sports", Icon: Volleyball },
];

const navClass = (active: boolean) =>
  cn(
    "flex w-full items-center justify-center gap-3 rounded-lg px-2 py-2.5 text-sm font-medium transition-colors md:justify-start md:px-3",
    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400",
    active ? "bg-sky-600/25 text-sky-100" : "text-slate-300 hover:bg-slate-800 hover:text-white",
  );

export function Layout() {
  const store = useStore();
  const navigate = useNavigate();
  const location = useLocation();
  const [filters, setFiltersState] = useState<Filters>(DEFAULT_FILTERS);
  const [view, setView] = useState<"poster" | "list">("poster");
  const [page, setPage] = useState(1);
  const [columns, setColumns] = useState(loadPosterColumns);
  const [settingsOpen, setSettingsOpen] = useState<Section | null>(null);
  const openSettings = (section: Section = "This browser") => setSettingsOpen(section);
  const [search, setSearch] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const firstPath = useRef(true);
  const setFilters = (f: Filters) => {
    setFiltersState(f);
    setPage(1);
  };
  const onBrowse = location.pathname === "/";

  useEffect(() => setSearch(store.query), [store.query]);

  // After navigating, move focus to the new page's heading so keyboard and
  // screen-reader users start there (each page sets its own title).
  useEffect(() => {
    if (firstPath.current) {
      firstPath.current = false;
      return;
    }
    const heading = contentRef.current?.querySelector<HTMLElement>("h1");
    (heading ?? contentRef.current)?.focus({ preventScroll: true });
  }, [location.pathname]);

  const submitSearch = (event: FormEvent) => {
    event.preventDefault();
    setPage(1);
    void store.loadShows(search);
    if (!onBrowse) navigate("/");
  };
  const reset = () => {
    setFilters(DEFAULT_FILTERS);
    setView("poster");
    setSearch("");
    void store.loadShows("");
    navigate("/");
  };

  const followedCount = store.follows.reduce((n, f) => n + f.sids.length, 0);
  const refreshing = store.catalogue?.state === "refreshing";
  const catalogueFailed = store.catalogue?.state === "failed";
  const ruvWarning = store.runStatus?.lastRunState === "warning" && /RÚV/.test(store.runStatus.lastRunMessage ?? "");

  return (
    <BrowseContext.Provider value={{ filters, setFilters, view, setView, page, setPage, columns, setColumns, openSettings }}>
      <div className="flex min-h-screen bg-slate-950 text-slate-100">
        <aside className="sticky top-0 flex h-dvh w-12 shrink-0 flex-col border-r border-slate-800 bg-slate-900 px-1 py-4 md:w-44 md:px-3">
          <button
            type="button"
            onClick={reset}
            aria-label="RÚV Sarpur: back to the start"
            className="mb-6 flex items-center justify-center gap-2 rounded-md hover:text-sky-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400 md:justify-start"
          >
            <Clapperboard className="h-6 w-6 shrink-0 text-sky-400" aria-hidden />
            <span className="sr-only text-base font-bold tracking-tight md:not-sr-only">RÚV Sarpur</span>
          </button>
          <nav aria-label="Catalogue" className="flex w-full flex-1 flex-col gap-1">
            {NAV.map(({ key, label, Icon }) => (
              <button
                key={key}
                type="button"
                aria-current={onBrowse && filters.contentType === key ? "page" : undefined}
                className={navClass(onBrowse && filters.contentType === key)}
                onClick={() => {
                  setFilters({ ...filters, contentType: key });
                  navigate("/");
                }}
              >
                <Icon className="h-5 w-5 shrink-0" aria-hidden />
                <span className="sr-only md:not-sr-only">{label}</span>
              </button>
            ))}
            <NavLink to="/following" className={({ isActive }) => navClass(isActive)}>
              <Heart className="h-5 w-5 shrink-0" aria-hidden />
              <span className="sr-only md:not-sr-only">Following</span>
            </NavLink>
          </nav>
          <button type="button" className={navClass(false)} onClick={() => openSettings()}>
            <SettingsIcon className="h-5 w-5 shrink-0" aria-hidden />
            <span className="sr-only md:not-sr-only">Settings</span>
          </button>
        </aside>

        <main className="flex min-w-0 flex-1 flex-col">
          <header className="flex flex-wrap items-center gap-2 border-b border-slate-800 bg-slate-900 px-3 py-3 md:gap-3 md:px-4">
            <form role="search" onSubmit={submitSearch} className="flex h-9 min-w-0 flex-1 basis-48 items-center overflow-hidden rounded-md border border-slate-500 bg-slate-800 focus-within:ring-2 focus-within:ring-sky-400">
              <Search className="ml-3 h-4 w-4 shrink-0 text-slate-300" aria-hidden />
              <Input
                ref={searchRef}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search by title"
                aria-label="Search by title"
                className="h-9 min-w-0 border-0 bg-transparent focus-visible:ring-0"
              />
              {search ? (
                <button
                  type="button"
                  aria-label="Clear the search"
                  className="mr-1 rounded-full p-1 text-slate-300 hover:bg-slate-700 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400"
                  onClick={() => {
                    searchRef.current?.focus();
                    setSearch("");
                    void store.loadShows("");
                  }}
                >
                  <XCircle className="h-4 w-4" aria-hidden />
                </button>
              ) : null}
              <Button
                type="submit"
                variant="secondary"
                size="sm"
                aria-disabled={store.showsLoading}
                className="mr-1 h-7 border-0 bg-slate-600 px-3 text-xs text-white hover:bg-slate-500"
              >
                Search
              </Button>
            </form>
            {onBrowse ? (
              <div className="flex shrink-0 items-center gap-1" role="group" aria-label="View">
                {(["list", "poster"] as const).map((v) => (
                  <Button
                    key={v}
                    size="sm"
                    variant="secondary"
                    aria-pressed={view === v}
                    onClick={() => setView(v)}
                    className={cn("h-9", view === v && "border-slate-100 bg-slate-100 text-slate-900 hover:bg-white")}
                  >
                    {v === "list" ? <Tv className="mr-2 h-4 w-4" aria-hidden /> : <Clapperboard className="mr-2 h-4 w-4" aria-hidden />}
                    {v === "list" ? "List" : "Posters"}
                  </Button>
                ))}
              </div>
            ) : null}
            <div className="hidden items-center gap-2 text-xs text-slate-200 lg:flex">
              <span className="rounded-full border border-slate-600 bg-slate-800 px-3 py-1">{store.shows.length} episodes</span>
              <span className="rounded-full border border-slate-600 bg-slate-800 px-3 py-1">{followedCount} followed</span>
              <span className={cn("rounded-full border px-3 py-1", store.runStatus?.running ? "border-sky-500/50 bg-sky-950 text-sky-100" : "border-slate-600 bg-slate-800")}>
                {store.runStatus?.running ? "Follow run active" : "Follows idle"}
              </span>
            </div>
            <Button size="sm" className="h-9 shrink-0" aria-disabled={refreshing} onClick={() => !refreshing && void store.refreshCatalogue()}>
              <RefreshCw className={cn("mr-2 h-4 w-4", refreshing && "motion-safe:animate-spin")} aria-hidden />
              {refreshing ? "Refreshing…" : "Refresh catalogue"}
            </Button>
          </header>

          <div className="space-y-2 px-3 pt-3 empty:hidden md:px-4">
            {store.health?.dryRun ? (
              <p role="status" className="rounded-lg border border-violet-500/50 bg-violet-950 px-4 py-2 text-sm text-violet-100">
                Dry run: sarpur logs what it would download and downloads nothing.
              </p>
            ) : null}
            {store.health?.update?.available ? (
              <p role="status" className="rounded-lg border border-sky-500/50 bg-sky-950 px-4 py-2 text-sm text-sky-100">
                sarpur {store.health.update.available} is out (this is {store.health.version}).{" "}
                {store.health.update.url ? (
                  <a className="rounded underline underline-offset-2 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400" href={store.health.update.url} target="_blank" rel="noreferrer">
                    What’s new
                  </a>
                ) : null}
              </p>
            ) : null}
            {catalogueFailed || ruvWarning ? (
              <p role="alert" className="rounded-lg border border-rose-500/50 bg-rose-950 px-4 py-2 text-sm text-rose-100">
                Couldn’t get the catalogue from RÚV{store.catalogue?.error ? `: ${store.catalogue.error}` : ""}. The next refresh or follow run tries again.
              </p>
            ) : null}
            {store.catalogue?.state === "loading" || (store.catalogue?.state === "refreshing" && store.shows.length === 0) ? (
              <p role="status" className="rounded-lg border border-sky-500/50 bg-sky-950 px-4 py-2 text-sm text-sky-100">
                {store.catalogue.state === "refreshing" ? "Fetching the catalogue from RÚV. The first time takes a few minutes." : "Loading the catalogue…"}
              </p>
            ) : null}
            {store.settings && !store.settings.autoEnabled && followedCount > 0 ? (
              <div role="status" className="flex flex-wrap items-center gap-3 rounded-lg border border-amber-500/50 bg-amber-950 px-4 py-2 text-sm text-amber-100">
                <span>Followed shows aren’t downloaded: automatic downloads are off.</span>
                <Button size="sm" className="h-7 px-3 text-xs" onClick={() => openSettings("Automatic downloads")}>
                  Turn on
                </Button>
              </div>
            ) : null}
            <NoticeBar onDismiss={() => contentRef.current?.focus({ preventScroll: true })} />
          </div>

          {/* pb-24: the download queue button never covers the last row. */}
          <div ref={contentRef} tabIndex={-1} className="flex-1 p-3 pb-24 focus:outline-none md:p-6 md:pb-24">
            <Outlet />
          </div>
        </main>

        <LiveNotice />
        <DownloadQueue />
        {settingsOpen ? <SettingsDialog initialSection={settingsOpen} onClose={() => setSettingsOpen(null)} /> : null}
      </div>
    </BrowseContext.Provider>
  );
}
